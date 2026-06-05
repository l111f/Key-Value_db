use crate::disk_manager::DiskManager;
use crate::error::{KvError, Result};

/// WAL 操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WALOpType {
    Put = 0x01,
    Delete = 0x02,
    Commit = 0x03,
    Rollback = 0x04,
}

impl WALOpType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(WALOpType::Put),
            0x02 => Some(WALOpType::Delete),
            0x03 => Some(WALOpType::Commit),
            0x04 => Some(WALOpType::Rollback),
            _ => None,
        }
    }

    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

/// WAL 日志记录
#[derive(Debug, Clone)]
pub struct WALRecord {
    /// 日志序列号，全局递增
    pub lsn: u64,
    /// 所属事务 ID
    pub txn_id: u64,
    /// 操作类型
    pub op_type: WALOpType,
    /// 操作的 Key
    pub key: Vec<u8>,
    /// 操作的 Value（Put 时有值）
    pub value: Option<Vec<u8>>,
    /// CRC32 校验和
    pub crc: u32,
}

/// CRC32 查找表
fn crc32(data: &[u8]) -> u32 {
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

/// WAL 记录二进制布局：
/// LSN: 8 字节
/// TxnID: 8 字节
/// OpType: 1 字节
/// KeyLen: 4 字节
/// Key: 可变
/// ValueLen: 4 字节 (0xFFFFFFFF = None)
/// Value: 可变
/// CRC: 4 字节

const VALUE_NONE_MARKER: u32 = 0xFFFFFFFF;

impl WALRecord {
    /// 序列化 WAL 记录为字节流（不含 CRC 尾部，需要单独计算）
    pub fn serialize_data(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // LSN
        buf.extend_from_slice(&self.lsn.to_le_bytes());
        // TxnID
        buf.extend_from_slice(&self.txn_id.to_le_bytes());
        // OpType
        buf.push(self.op_type.to_byte());
        // KeyLen + Key
        buf.extend_from_slice(&(self.key.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.key);
        // ValueLen + Value
        match &self.value {
            Some(v) => {
                buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
                buf.extend_from_slice(v);
            }
            None => {
                buf.extend_from_slice(&VALUE_NONE_MARKER.to_le_bytes());
            }
        }

        buf
    }

    /// 完整序列化（含 CRC）
    pub fn serialize(&self) -> Vec<u8> {
        let data = self.serialize_data();
        let crc = crc32(&data);
        let mut result = data;
        result.extend_from_slice(&crc.to_le_bytes());
        result
    }

    /// 从字节流反序列化，返回 (WALRecord, consumed_bytes)
    pub fn deserialize(data: &[u8]) -> Result<(WALRecord, usize)> {
        let mut offset = 0;

        // LSN: 8 字节
        if offset + 8 > data.len() {
            return Err(KvError::WalCorrupted { lsn: 0 });
        }
        let lsn = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        offset += 8;

        // TxnID: 8 字节
        if offset + 8 > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let txn_id = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        offset += 8;

        // OpType: 1 字节
        if offset + 1 > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let op_type = WALOpType::from_byte(data[offset]).ok_or(KvError::WalCorrupted { lsn })?;
        offset += 1;

        // KeyLen: 4 字节
        if offset + 4 > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let key_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        // Key
        if offset + key_len > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let key = data[offset..offset + key_len].to_vec();
        offset += key_len;

        // ValueLen: 4 字节
        if offset + 4 > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let value_len_raw = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let value = if value_len_raw == VALUE_NONE_MARKER {
            None
        } else {
            let value_len = value_len_raw as usize;
            if offset + value_len > data.len() {
                return Err(KvError::WalCorrupted { lsn });
            }
            let v = data[offset..offset + value_len].to_vec();
            offset += value_len;
            Some(v)
        };

        // CRC: 4 字节
        if offset + 4 > data.len() {
            return Err(KvError::WalCorrupted { lsn });
        }
        let stored_crc = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        offset += 4;

        // 验证 CRC
        let computed_crc = crc32(&data[..offset - 4]);
        if stored_crc != computed_crc {
            return Err(KvError::WalCorrupted { lsn });
        }

        Ok((
            WALRecord {
                lsn,
                txn_id,
                op_type,
                key,
                value,
                crc: stored_crc,
            },
            offset,
        ))
    }
}

/// WAL 管理器
pub struct WALManager {
    /// 磁盘管理器（用于 WAL 文件读写）
    disk_manager: DiskManager,
    /// 当前 LSN
    current_lsn: u64,
}

impl WALManager {
    /// 初始化，读取现有 WAL 文件确定 current_lsn 起始值
    pub fn new(disk_manager: DiskManager) -> Result<Self> {
        let mut mgr = WALManager {
            disk_manager,
            current_lsn: 1,
        };

        // 读取现有 WAL 记录以确定 LSN 起始值
        let wal_data = mgr.disk_manager.read_wal()?;
        if !wal_data.is_empty() {
            let mut offset = 0;
            let mut max_lsn = 0u64;
            while offset < wal_data.len() {
                match WALRecord::deserialize(&wal_data[offset..]) {
                    Ok((record, consumed)) => {
                        max_lsn = max_lsn.max(record.lsn);
                        offset += consumed;
                    }
                    Err(_) => {
                        // 损坏记录，停止读取
                        break;
                    }
                }
            }
            mgr.current_lsn = max_lsn + 1;
        }

        Ok(mgr)
    }

    /// 追加 WAL 记录，设置 current_lsn，序列化并写入，fsync，返回 LSN
    pub fn append(&mut self, mut record: WALRecord) -> Result<u64> {
        let lsn = self.current_lsn;
        record.lsn = lsn;
        self.current_lsn += 1;

        let serialized = record.serialize();
        self.disk_manager.write_wal(&serialized)?;

        Ok(lsn)
    }

    /// 强制 fsync 当前 WAL 文件
    pub fn sync(&mut self) -> Result<()> {
        self.disk_manager.fsync()
    }

    /// 从 WAL 文件起始位置逐条读取并反序列化记录
    /// CRC 校验失败时停止读取，返回已成功校验的记录列表
    pub fn recover(&mut self) -> Result<Vec<WALRecord>> {
        let wal_data = self.disk_manager.read_wal()?;
        let mut records = Vec::new();
        let mut offset = 0;

        while offset < wal_data.len() {
            match WALRecord::deserialize(&wal_data[offset..]) {
                Ok((record, consumed)) => {
                    offset += consumed;
                    records.push(record);
                }
                Err(_) => {
                    // 损坏记录，停止读取
                    break;
                }
            }
        }

        Ok(records)
    }

    /// 清空或截断 WAL 文件
    pub fn clear(&mut self) -> Result<()> {
        self.disk_manager.clear_wal()
    }

    /// 获取磁盘管理器的可变引用
    pub fn get_disk_manager_mut(&mut self) -> &mut DiskManager {
        &mut self.disk_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    fn create_test_wal_manager() -> WALManager {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let dm = DiskManager::new(&db_path).unwrap();
        WALManager::new(dm).unwrap()
    }

    #[test]
    fn test_wal_record_serialize_deserialize() {
        let record = WALRecord {
            lsn: 1,
            txn_id: 100,
            op_type: WALOpType::Put,
            key: b"test_key".to_vec(),
            value: Some(b"test_value".to_vec()),
            crc: 0,
        };

        let serialized = record.serialize();
        let (deserialized, consumed) = WALRecord::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.lsn, 1);
        assert_eq!(deserialized.txn_id, 100);
        assert_eq!(deserialized.op_type, WALOpType::Put);
        assert_eq!(deserialized.key, b"test_key");
        assert_eq!(deserialized.value, Some(b"test_value".to_vec()));
        assert_eq!(consumed, serialized.len());
    }

    #[test]
    fn test_wal_record_delete_no_value() {
        let record = WALRecord {
            lsn: 2,
            txn_id: 101,
            op_type: WALOpType::Delete,
            key: b"del_key".to_vec(),
            value: None,
            crc: 0,
        };

        let serialized = record.serialize();
        let (deserialized, _) = WALRecord::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.op_type, WALOpType::Delete);
        assert_eq!(deserialized.value, None);
    }

    #[test]
    fn test_wal_corrupted_record() {
        let record = WALRecord {
            lsn: 3,
            txn_id: 102,
            op_type: WALOpType::Commit,
            key: Vec::new(),
            value: None,
            crc: 0,
        };

        let mut serialized = record.serialize();
        // 篡改数据
        if serialized.len() > 5 {
            serialized[3] ^= 0xFF;
        }

        let result = WALRecord::deserialize(&serialized);
        assert!(result.is_err());
    }

    #[test]
    fn test_wal_append_and_recover() {
        let mut mgr = create_test_wal_manager();

        let r1 = WALRecord {
            lsn: 0, txn_id: 1, op_type: WALOpType::Put,
            key: b"key1".to_vec(), value: Some(b"value1".to_vec()), crc: 0,
        };
        let r2 = WALRecord {
            lsn: 0, txn_id: 1, op_type: WALOpType::Put,
            key: b"key2".to_vec(), value: Some(b"value2".to_vec()), crc: 0,
        };
        let r3 = WALRecord {
            lsn: 0, txn_id: 1, op_type: WALOpType::Commit,
            key: Vec::new(), value: None, crc: 0,
        };

        mgr.append(r1).unwrap();
        mgr.append(r2).unwrap();
        mgr.append(r3).unwrap();

        let records = mgr.recover().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].lsn, 1);
        assert_eq!(records[1].lsn, 2);
        assert_eq!(records[2].lsn, 3);
        assert_eq!(records[2].op_type, WALOpType::Commit);
    }

    #[test]
    fn test_wal_clear() {
        let mut mgr = create_test_wal_manager();

        let record = WALRecord {
            lsn: 0, txn_id: 1, op_type: WALOpType::Commit,
            key: Vec::new(), value: None, crc: 0,
        };
        mgr.append(record).unwrap();

        mgr.clear().unwrap();
        let records = mgr.recover().unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_wal_lsn_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();

        let lsn;
        {
            let mut mgr = WALManager::new(DiskManager::new(&db_path).unwrap()).unwrap();
            let record = WALRecord {
                lsn: 0, txn_id: 1, op_type: WALOpType::Put,
                key: b"key".to_vec(), value: Some(b"value".to_vec()), crc: 0,
            };
            lsn = mgr.append(record).unwrap();
        }

        {
            let mut mgr = WALManager::new(DiskManager::new(&db_path).unwrap()).unwrap();
            let record = WALRecord {
                lsn: 0, txn_id: 2, op_type: WALOpType::Put,
                key: b"key2".to_vec(), value: Some(b"value2".to_vec()), crc: 0,
            };
            let new_lsn = mgr.append(record).unwrap();
            assert_eq!(new_lsn, lsn + 1);
        }
    }
}
