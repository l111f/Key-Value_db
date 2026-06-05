use std::fmt;

/// KV 存储引擎全局统一错误类型
#[derive(Debug)]
pub enum KvError {
    /// 数据页 CRC 校验失败
    PageCorrupted { page_id: u32 },
    /// 页不存在
    PageNotFound { page_id: u32 },
    /// 键不存在（非错误场景，用于 Get/Delete 返回指示）
    KeyNotFound,
    /// 文件 I/O 错误
    IoError(std::io::Error),
    /// WAL 记录校验失败
    WalCorrupted { lsn: u64 },
    /// 无活跃事务时调用 Commit/Rollback
    NoActiveTransaction,
    /// 嵌套事务不支持
    NestedTransaction,
    /// 数据库已关闭
    DatabaseClosed,
    /// 缓冲区已满，无法分配新帧
    BufferPoolFull,
}

impl fmt::Display for KvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KvError::PageCorrupted { page_id } => {
                write!(f, "Page corrupted: CRC check failed for page {}", page_id)
            }
            KvError::PageNotFound { page_id } => {
                write!(f, "Page not found: page {}", page_id)
            }
            KvError::KeyNotFound => {
                write!(f, "Key not found")
            }
            KvError::IoError(err) => {
                write!(f, "I/O error: {}", err)
            }
            KvError::WalCorrupted { lsn } => {
                write!(f, "WAL corrupted: CRC check failed at LSN {}", lsn)
            }
            KvError::NoActiveTransaction => {
                write!(f, "No active transaction")
            }
            KvError::NestedTransaction => {
                write!(f, "Nested transaction is not supported")
            }
            KvError::DatabaseClosed => {
                write!(f, "Database is closed")
            }
            KvError::BufferPoolFull => {
                write!(f, "Buffer pool is full, no available frame for eviction")
            }
        }
    }
}

impl std::error::Error for KvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KvError::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for KvError {
    fn from(err: std::io::Error) -> Self {
        KvError::IoError(err)
    }
}

/// 全局 Result 类型别名
pub type Result<T> = std::result::Result<T, KvError>;
