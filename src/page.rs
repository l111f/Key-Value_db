use crate::error::{KvError, Result};

/// 页大小：4KB
pub const PAGE_SIZE: usize = 4096;

/// 页内布局偏移量常量
pub const PAGE_TYPE_OFFSET: usize = 0;    // 1 字节
pub const CRC_OFFSET: usize = 1;          // 4 字节
pub const KEY_COUNT_OFFSET: usize = 5;    // 2 字节
pub const RESERVED_OFFSET: usize = 7;     // 1 字节
pub const DATA_OFFSET: usize = 8;         // 数据区起始

/// 页类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageType {
    /// 空闲页
    Free = 0x00,
    /// 内部节点
    Internal = 0x01,
    /// 叶子节点
    Leaf = 0x02,
}

impl PageType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(PageType::Free),
            0x01 => Some(PageType::Internal),
            0x02 => Some(PageType::Leaf),
            _ => None,
        }
    }

    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

/// 数据页结构体
#[derive(Debug, Clone)]
pub struct Page {
    /// 页号，全局唯一标识符
    pub page_id: u32,
    /// 页数据区，固定 4KB
    pub data: [u8; PAGE_SIZE],
    /// CRC32 校验和
    pub crc: u32,
    /// 脏标记，标识页是否被修改但未刷写
    pub is_dirty: bool,
}

impl Page {
    /// 创建空白页（数据区全零、类型为 Free）
    pub fn new(page_id: u32) -> Self {
        let mut page = Page {
            page_id,
            data: [0u8; PAGE_SIZE],
            crc: 0,
            is_dirty: false,
        };
        // 设置页类型为 Free
        page.set_page_type(PageType::Free);
        // 设置初始键数量为 0
        page.set_key_count(0);
        // 保留字节置零
        page.data[RESERVED_OFFSET] = 0;
        page
    }

    /// 计算偏移 8 至页尾数据的 CRC32 校验和
    pub fn compute_crc(&self) -> u32 {
        crc32(&self.data[DATA_OFFSET..PAGE_SIZE])
    }

    /// 校验存储的 CRC 与计算值一致，失败返回 KvError::PageCorrupted
    pub fn validate_crc(&self) -> Result<()> {
        let computed = self.compute_crc();
        if self.crc == computed {
            Ok(())
        } else {
            Err(KvError::PageCorrupted {
                page_id: self.page_id,
            })
        }
    }

    /// 将 Page 结构体序列化为 [u8; PAGE_SIZE] 字节数组
    /// 包含头部和数据区，最后更新 CRC 字段
    pub fn serialize(&mut self) -> [u8; PAGE_SIZE] {
        // 先计算并写入 CRC 到数据区的头部 CRC 位置
        let crc = self.compute_crc();
        self.crc = crc;
        self.data[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());

        self.data
    }

    /// 从字节数组解析为 Page 结构体，校验 CRC
    pub fn deserialize(page_id: u32, data: &[u8; PAGE_SIZE]) -> Result<Self> {
        let crc_bytes = &data[CRC_OFFSET..CRC_OFFSET + 4];
        let stored_crc = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);

        let page = Page {
            page_id,
            data: *data,
            crc: stored_crc,
            is_dirty: false,
        };

        // 首次创建或全零页时跳过 CRC 校验（CRC 为 0 且数据区全零）
        let computed_crc = page.compute_crc();
        if stored_crc == 0 && computed_crc == 0 {
            return Ok(page);
        }

        page.validate_crc()?;
        Ok(page)
    }

    /// 读取页类型标记
    pub fn get_page_type(&self) -> PageType {
        PageType::from_byte(self.data[PAGE_TYPE_OFFSET]).unwrap_or(PageType::Free)
    }

    /// 写入页类型标记
    pub fn set_page_type(&mut self, page_type: PageType) {
        self.data[PAGE_TYPE_OFFSET] = page_type.to_byte();
        self.is_dirty = true;
    }

    /// 读取键数量字段
    pub fn get_key_count(&self) -> u16 {
        let bytes = &self.data[KEY_COUNT_OFFSET..KEY_COUNT_OFFSET + 2];
        u16::from_le_bytes([bytes[0], bytes[1]])
    }

    /// 写入键数量字段
    pub fn set_key_count(&mut self, count: u16) {
        self.data[KEY_COUNT_OFFSET..KEY_COUNT_OFFSET + 2].copy_from_slice(&count.to_le_bytes());
        self.is_dirty = true;
    }
}

/// 简易 CRC32 计算（使用标准的 CRC32 算法）
/// 使用查表法实现 CRC-32 (ISO 3309 / ITU-T V.42)
fn crc32(data: &[u8]) -> u32 {
    // CRC32 查找表（多项式 0xEDB88320）
    static CRC_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };

    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        let index = ((crc ^ (*byte as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC_TABLE[index];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_new() {
        let page = Page::new(0);
        assert_eq!(page.page_id, 0);
        assert_eq!(page.get_page_type(), PageType::Free);
        assert_eq!(page.get_key_count(), 0);
    }

    #[test]
    fn test_page_type() {
        let mut page = Page::new(1);
        page.set_page_type(PageType::Leaf);
        assert_eq!(page.get_page_type(), PageType::Leaf);

        page.set_page_type(PageType::Internal);
        assert_eq!(page.get_page_type(), PageType::Internal);
    }

    #[test]
    fn test_key_count() {
        let mut page = Page::new(2);
        page.set_key_count(42);
        assert_eq!(page.get_key_count(), 42);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut page = Page::new(3);
        page.set_page_type(PageType::Leaf);
        page.set_key_count(5);
        // 在数据区写入一些测试数据
        page.data[DATA_OFFSET..DATA_OFFSET + 4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let serialized = page.serialize();
        let deserialized = Page::deserialize(3, &serialized).unwrap();

        assert_eq!(deserialized.page_id, 3);
        assert_eq!(deserialized.get_page_type(), PageType::Leaf);
        assert_eq!(deserialized.get_key_count(), 5);
        assert_eq!(&deserialized.data[DATA_OFFSET..DATA_OFFSET + 4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_crc_validation_success() {
        let mut page = Page::new(4);
        page.set_page_type(PageType::Internal);
        page.set_key_count(3);
        let _ = page.serialize();
        assert!(page.validate_crc().is_ok());
    }

    #[test]
    fn test_crc_validation_failure() {
        let mut page = Page::new(5);
        page.set_page_type(PageType::Leaf);
        page.set_key_count(1);
        let _ = page.serialize();
        // 篡改数据区
        page.data[DATA_OFFSET] = 0xFF;
        assert!(page.validate_crc().is_err());
    }

    #[test]
    fn test_crc32_known_values() {
        // 空数据
        assert_eq!(crc32(&[]), 0x00000000);
        // "123456789" 的标准 CRC32 值
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }
}
