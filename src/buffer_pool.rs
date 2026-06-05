use std::collections::HashMap;
use std::collections::VecDeque;

use crate::disk_manager::DiskManager;
use crate::error::{KvError, Result};
use crate::page::{Page, PAGE_SIZE};

/// 缓冲帧：缓冲池中的单个页缓存
#[derive(Debug)]
pub struct BufferFrame {
    /// 缓存的页号
    pub page_id: u32,
    /// 页数据副本
    pub data: [u8; PAGE_SIZE],
    /// 是否被修改
    pub is_dirty: bool,
    /// 引用计数，>0 时不允许淘汰
    pub pin_count: u32,
    /// 最近访问时间戳，LRU 排序依据
    pub last_access: u64,
}

/// 缓冲池管理器：内存缓冲区管理，LRU 淘汰，脏页刷写
pub struct BufferPoolManager {
    /// 缓冲帧集合
    frames: HashMap<u32, BufferFrame>,
    /// 双向队列维护 LRU 顺序
    lru_order: VecDeque<u32>,
    /// 缓冲池容量（最大帧数）
    capacity: usize,
    /// 磁盘管理器
    disk_manager: DiskManager,
    /// 全局访问计数器，用于 LRU 时间戳
    access_counter: u64,
}

impl BufferPoolManager {
    /// 初始化指定容量的缓冲池
    pub fn new(capacity: usize, disk_manager: DiskManager) -> Self {
        BufferPoolManager {
            frames: HashMap::new(),
            lru_order: VecDeque::new(),
            capacity,
            disk_manager,
            access_counter: 0,
        }
    }

    /// 获取下一个访问时间戳
    fn next_access_time(&mut self) -> u64 {
        self.access_counter += 1;
        self.access_counter
    }

    /// 更新 LRU 顺序：将 page_id 移到前端（最近使用）
    fn update_lru(&mut self, page_id: u32) {
        // 从当前位置移除
        self.lru_order.retain(|&id| id != page_id);
        // 插入到前端
        self.lru_order.push_front(page_id);
    }

    /// 获取数据页，优先从 frames 中查找
    /// 未命中时调用 DiskManager::read_page 加载到缓冲区
    /// 若缓冲区满则触发淘汰
    pub fn fetch_page(&mut self, page_id: u32) -> Result<Page> {
        // 检查缓冲区中是否已有
        if self.frames.contains_key(&page_id) {
            let access_time = self.next_access_time();
            let frame = self.frames.get_mut(&page_id).unwrap();
            frame.pin_count += 1;
            frame.last_access = access_time;

            let page = Page {
                page_id: frame.page_id,
                data: frame.data,
                crc: 0,
                is_dirty: frame.is_dirty,
            };
            // 更新 LRU（先释放 borrow）
            self.update_lru(page_id);
            return Ok(page);
        }

        // 缓冲区未命中，需要从磁盘加载
        if self.frames.len() >= self.capacity {
            self.evict()?;
        }

        // 从磁盘读取
        let data = self.disk_manager.read_page(page_id)?;
        let access_time = self.next_access_time();

        let frame = BufferFrame {
            page_id,
            data,
            is_dirty: false,
            pin_count: 1,
            last_access: access_time,
        };

        self.frames.insert(page_id, frame);
        self.update_lru(page_id);

        // 构造返回的 Page
        let frame = self.frames.get(&page_id).unwrap();
        let page = Page {
            page_id: frame.page_id,
            data: frame.data,
            crc: 0,
            is_dirty: frame.is_dirty,
        };
        Ok(page)
    }

    /// 分配新页号，创建空白 BufferFrame 并加入缓冲区
    pub fn create_page(&mut self) -> Result<(u32, Page)> {
        // 分配新页号
        let page_id = self.disk_manager.alloc_page()?;

        // 检查是否需要淘汰
        if self.frames.len() >= self.capacity {
            // 新页已经在磁盘上分配，尝试淘汰
            if let Err(e) = self.evict() {
                // 淘汰失败，但页已分配，这不影响正确性
                return Err(e);
            }
        }

        let access_time = self.next_access_time();
        let data = [0u8; PAGE_SIZE];

        let frame = BufferFrame {
            page_id,
            data,
            is_dirty: true, // 新创建的页标记为脏页
            pin_count: 1,
            last_access: access_time,
        };

        self.frames.insert(page_id, frame);
        self.update_lru(page_id);

        let frame = self.frames.get(&page_id).unwrap();
        let page = Page {
            page_id: frame.page_id,
            data: frame.data,
            crc: 0,
            is_dirty: frame.is_dirty,
        };
        Ok((page_id, page))
    }

    /// 标记指定帧为脏页
    pub fn mark_dirty(&mut self, page_id: u32) {
        if let Some(frame) = self.frames.get_mut(&page_id) {
            frame.is_dirty = true;
        }
    }

    /// 将页数据写回缓冲帧（更新缓冲区中的数据）
    pub fn write_page_data(&mut self, page_id: u32, data: &[u8; PAGE_SIZE]) -> Result<()> {
        if let Some(frame) = self.frames.get_mut(&page_id) {
            frame.data.copy_from_slice(data);
            frame.is_dirty = true;
            Ok(())
        } else {
            Err(KvError::PageNotFound { page_id })
        }
    }

    /// 获取缓冲区中页的引用数据（不增加 pin_count）
    pub fn get_page_data(&self, page_id: u32) -> Option<&[u8; PAGE_SIZE]> {
        self.frames.get(&page_id).map(|f| &f.data)
    }

    /// 获取缓冲区中页的可变引用数据（不增加 pin_count）
    pub fn get_page_data_mut(&mut self, page_id: u32) -> Option<&mut [u8; PAGE_SIZE]> {
        if let Some(frame) = self.frames.get_mut(&page_id) {
            frame.is_dirty = true;
            Some(&mut frame.data)
        } else {
            None
        }
    }

    /// 若帧为脏页，调用 DiskManager::write_page 写入磁盘并调用 fsync，清除脏标记
    pub fn flush_page(&mut self, page_id: u32) -> Result<()> {
        if let Some(frame) = self.frames.get_mut(&page_id) {
            if frame.is_dirty {
                self.disk_manager.write_page(page_id, &frame.data)?;
                frame.is_dirty = false;
            }
        }
        Ok(())
    }

    /// 遍历所有帧，刷写所有脏页
    pub fn flush_all(&mut self) -> Result<()> {
        let page_ids: Vec<u32> = self.frames.keys().cloned().collect();
        for page_id in page_ids {
            self.flush_page(page_id)?;
        }
        self.disk_manager.fsync()?;
        Ok(())
    }

    /// 递减 pin_count，允许该帧被淘汰
    pub fn unpin(&mut self, page_id: u32) {
        if let Some(frame) = self.frames.get_mut(&page_id) {
            if frame.pin_count > 0 {
                frame.pin_count -= 1;
            }
        }
    }

    /// 获取页的 pin_count
    pub fn get_pin_count(&self, page_id: u32) -> u32 {
        self.frames
            .get(&page_id)
            .map(|f| f.pin_count)
            .unwrap_or(0)
    }

    /// LRU 淘汰逻辑
    /// 从 lru_order 尾部查找 pin_count == 0 的帧
    /// 脏页先调用 flush_page 刷写再移除
    /// 无可用帧时返回错误
    pub fn evict(&mut self) -> Result<()> {
        // 从 LRU 尾部开始查找可淘汰的帧
        let mut evict_candidate: Option<u32> = None;

        for &page_id in self.lru_order.iter().rev() {
            if let Some(frame) = self.frames.get(&page_id) {
                if frame.pin_count == 0 {
                    evict_candidate = Some(page_id);
                    break;
                }
            }
        }

        match evict_candidate {
            Some(page_id) => {
                // 如果是脏页，先刷写
                let is_dirty = self.frames.get(&page_id).map(|f| f.is_dirty).unwrap_or(false);
                if is_dirty {
                    self.flush_page(page_id)?;
                }

                // 从缓冲区和 LRU 队列中移除
                self.frames.remove(&page_id);
                self.lru_order.retain(|&id| id != page_id);
                Ok(())
            }
            None => Err(KvError::BufferPoolFull),
        }
    }

    /// 调用 flush_all 刷写所有脏页，清空缓冲区
    pub fn shutdown(&mut self) -> Result<()> {
        self.flush_all()?;
        self.frames.clear();
        self.lru_order.clear();
        Ok(())
    }

    /// 获取磁盘管理器的可变引用
    pub fn get_disk_manager_mut(&mut self) -> &mut DiskManager {
        &mut self.disk_manager
    }

    /// 获取磁盘管理器的不可变引用
    pub fn get_disk_manager_ref(&self) -> &DiskManager {
        &self.disk_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    fn create_test_bpm(capacity: usize) -> BufferPoolManager {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        // 将 dir 保存到线程局部或全局防止提前 drop
        // 这里简化处理，使用固定路径
        let dm = DiskManager::new(&db_path).unwrap();
        BufferPoolManager::new(capacity, dm)
    }

    #[test]
    fn test_create_page() {
        let mut bpm = create_test_bpm(10);
        let (page_id, page) = bpm.create_page().unwrap();
        assert_eq!(page_id, 0);
        assert_eq!(page.data, [0u8; PAGE_SIZE]);
    }

    #[test]
    fn test_fetch_page() {
        let mut bpm = create_test_bpm(10);
        let (page_id, _) = bpm.create_page().unwrap();
        bpm.unpin(page_id);

        // 将数据写入缓冲帧
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 42;
        bpm.write_page_data(page_id, &data).unwrap();
        bpm.flush_page(page_id).unwrap();

        // 移除缓冲帧后重新获取
        bpm.frames.remove(&page_id);
        bpm.lru_order.retain(|&id| id != page_id);

        let page = bpm.fetch_page(page_id).unwrap();
        assert_eq!(page.data[0], 42);
    }

    #[test]
    fn test_mark_dirty_and_flush() {
        let mut bpm = create_test_bpm(10);
        let (page_id, _) = bpm.create_page().unwrap();
        bpm.unpin(page_id);

        bpm.mark_dirty(page_id);
        bpm.flush_page(page_id).unwrap();

        // 脏标记应被清除
        let frame = bpm.frames.get(&page_id).unwrap();
        assert!(!frame.is_dirty);
    }

    #[test]
    fn test_eviction() {
        let mut bpm = create_test_bpm(2);

        // 创建 3 个页，超出容量
        let (p0, _) = bpm.create_page().unwrap();
        let (p1, _) = bpm.create_page().unwrap();

        bpm.unpin(p0);
        bpm.unpin(p1);

        // 创建第三个页时应触发淘汰
        let (p2, _) = bpm.create_page().unwrap();
        assert!(bpm.frames.contains_key(&p2));
    }

    #[test]
    fn test_lru_order() {
        let mut bpm = create_test_bpm(3);

        let (p0, _) = bpm.create_page().unwrap();
        let (p1, _) = bpm.create_page().unwrap();
        let (p2, _) = bpm.create_page().unwrap();

        bpm.unpin(p0);
        bpm.unpin(p1);
        bpm.unpin(p2);

        // 访问 p0，使其成为最近使用
        let _ = bpm.fetch_page(p0).unwrap();
        bpm.unpin(p0);

        // 淘汰应移除 p1（最久未使用）
        bpm.evict().unwrap();
        assert!(!bpm.frames.contains_key(&p1));
        assert!(bpm.frames.contains_key(&p0));
        assert!(bpm.frames.contains_key(&p2));
    }

    #[test]
    fn test_pinned_cannot_evict() {
        let mut bpm = create_test_bpm(2);

        let (p0, _) = bpm.create_page().unwrap();
        let (p1, _) = bpm.create_page().unwrap();

        // p0 和 p1 都被 pin，无法淘汰
        // 注意 create_page 会设置 pin_count = 1
        let result = bpm.evict();
        assert!(result.is_err());
    }

    #[test]
    fn test_unpin() {
        let mut bpm = create_test_bpm(10);
        let (page_id, _) = bpm.create_page().unwrap();

        assert_eq!(bpm.get_pin_count(page_id), 1);
        bpm.unpin(page_id);
        assert_eq!(bpm.get_pin_count(page_id), 0);
    }

    #[test]
    fn test_shutdown() {
        let mut bpm = create_test_bpm(10);
        let (page_id, _) = bpm.create_page().unwrap();
        bpm.mark_dirty(page_id);

        bpm.shutdown().unwrap();
        assert!(bpm.frames.is_empty());
        assert!(bpm.lru_order.is_empty());
    }
}