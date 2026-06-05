use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::Result;
use crate::page::PAGE_SIZE;

/// 磁盘管理器，负责数据文件和 WAL 文件的物理读写
pub struct DiskManager {
    /// 数据文件句柄
    db_file: File,
    /// WAL 文件句柄
    wal_file: File,
    /// 下一个可分配的页 ID
    next_page_id: u32,
    /// 数据库文件路径
    db_path: String,
    /// WAL 文件路径
    wal_path: String,
}

impl DiskManager {
    /// 创建或打开 .db 数据文件和 .wal 日志文件
    /// 根据 .db 文件大小计算已有页数，初始化 next_page_id
    pub fn new(db_path: &str) -> Result<Self> {
        let wal_path = db_path.replace(".db", ".wal");

        // 确保目录存在
        if let Some(parent) = Path::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // 确保目录存在
        if let Some(parent) = Path::new(&wal_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // 打开或创建数据文件（读写模式）
        let db_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(db_path)?;

        // 打开或创建 WAL 文件（读写模式）
        let wal_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&wal_path)?;

        // 根据文件大小计算已有页数
        let file_size = db_file.metadata()?.len();
        let num_pages = (file_size / PAGE_SIZE as u64) as u32;

        Ok(DiskManager {
            db_file,
            wal_file,
            next_page_id: num_pages,
            db_path: db_path.to_string(),
            wal_path,
        })
    }

    /// 计算文件偏移 page_id * 4096，使用 Seek + Read 精确读取 4KB 数据
    pub fn read_page(&mut self, page_id: u32) -> Result<[u8; PAGE_SIZE]> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.db_file.seek(SeekFrom::Start(offset))?;

        let mut buffer = [0u8; PAGE_SIZE];
        let mut total_read = 0;
        while total_read < PAGE_SIZE {
            let bytes_read = self.db_file.read(&mut buffer[total_read..])?;
            if bytes_read == 0 {
                // EOF 已到达但数据不足，说明页不存在
                break;
            }
            total_read += bytes_read;
        }

        if total_read < PAGE_SIZE {
            // 文件不足一页，填充零（扩展文件）
            // 将当前内容写回并补零
            self.db_file.seek(SeekFrom::Start(offset))?;
            self.db_file.write_all(&buffer)?;
        }

        Ok(buffer)
    }

    /// 计算偏移并使用 Seek + Write 写入 4KB 数据
    pub fn write_page(&mut self, page_id: u32, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.db_file.seek(SeekFrom::Start(offset))?;
        self.db_file.write_all(data)?;
        Ok(())
    }

    /// 返回当前 next_page_id 并递增；若文件不足则扩展（写入零填充页）
    pub fn alloc_page(&mut self) -> Result<u32> {
        let page_id = self.next_page_id;
        self.next_page_id += 1;

        // 扩展文件：写入零填充页
        let zero_page = [0u8; PAGE_SIZE];
        self.write_page(page_id, &zero_page)?;

        Ok(page_id)
    }

    /// 调用 db_file.sync_all() 和 wal_file.sync_all() 确保数据落盘
    pub fn fsync(&mut self) -> Result<()> {
        self.db_file.sync_all()?;
        self.wal_file.sync_all()?;
        Ok(())
    }

    /// 执行 fsync 后关闭文件句柄
    pub fn shutdown(&mut self) -> Result<()> {
        self.fsync()?;
        Ok(())
    }

    /// 获取当前已分配的页数
    pub fn get_num_pages(&self) -> u32 {
        self.next_page_id
    }

    /// 读取 WAL 文件全部内容
    pub fn read_wal(&mut self) -> Result<Vec<u8>> {
        self.wal_file.seek(SeekFrom::Start(0))?;
        let mut data = Vec::new();
        self.wal_file.read_to_end(&mut data)?;
        Ok(data)
    }

    /// 追加写入 WAL 数据
    pub fn write_wal(&mut self, data: &[u8]) -> Result<()> {
        self.wal_file.seek(SeekFrom::End(0))?;
        self.wal_file.write_all(data)?;
        self.wal_file.sync_all()?;
        Ok(())
    }

    /// 清空/截断 WAL 文件
    pub fn clear_wal(&mut self) -> Result<()> {
        self.wal_file.set_len(0)?;
        self.wal_file.seek(SeekFrom::Start(0))?;
        self.wal_file.sync_all()?;
        Ok(())
    }

    /// 获取数据文件路径
    pub fn get_db_path(&self) -> &str {
        &self.db_path
    }

    /// 获取 WAL 文件路径
    pub fn get_wal_path(&self) -> &str {
        &self.wal_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    #[test]
    fn test_disk_manager_new() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let dm = DiskManager::new(&db_path).unwrap();
        assert_eq!(dm.get_num_pages(), 0);
    }

    #[test]
    fn test_alloc_and_read_page() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let mut dm = DiskManager::new(&db_path).unwrap();

        let page_id = dm.alloc_page().unwrap();
        assert_eq!(page_id, 0);
        assert_eq!(dm.get_num_pages(), 1);

        let data = dm.read_page(page_id).unwrap();
        assert_eq!(data, [0u8; PAGE_SIZE]);
    }

    #[test]
    fn test_write_and_read_page() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let mut dm = DiskManager::new(&db_path).unwrap();

        let page_id = dm.alloc_page().unwrap();
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 42;
        data[1] = 99;
        dm.write_page(page_id, &data).unwrap();

        let read_data = dm.read_page(page_id).unwrap();
        assert_eq!(read_data[0], 42);
        assert_eq!(read_data[1], 99);
    }

    #[test]
    fn test_multiple_alloc() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let mut dm = DiskManager::new(&db_path).unwrap();

        let id0 = dm.alloc_page().unwrap();
        let id1 = dm.alloc_page().unwrap();
        let id2 = dm.alloc_page().unwrap();

        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(dm.get_num_pages(), 3);
    }

    #[test]
    fn test_wal_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let mut dm = DiskManager::new(&db_path).unwrap();

        dm.write_wal(b"hello").unwrap();
        dm.write_wal(b" world").unwrap();

        let wal_data = dm.read_wal().unwrap();
        assert_eq!(wal_data, b"hello world");
    }

    #[test]
    fn test_wal_clear() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let mut dm = DiskManager::new(&db_path).unwrap();

        dm.write_wal(b"some log data").unwrap();
        dm.clear_wal().unwrap();

        let wal_data = dm.read_wal().unwrap();
        assert!(wal_data.is_empty());
    }

    #[test]
    fn test_reopen_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();

        {
            let mut dm = DiskManager::new(&db_path).unwrap();
            let page_id = dm.alloc_page().unwrap();
            let mut data = [0u8; PAGE_SIZE];
            data[0] = 123;
            dm.write_page(page_id, &data).unwrap();
            dm.fsync().unwrap();
        }

        {
            let mut dm = DiskManager::new(&db_path).unwrap();
            assert_eq!(dm.get_num_pages(), 1);
            let data = dm.read_page(0).unwrap();
            assert_eq!(data[0], 123);
        }
    }
}
