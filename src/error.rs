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
    /// 键大小超过限制
    KeyTooLarge { key_size: usize, max_size: usize },
    /// 值大小超过限制
    ValueTooLarge { value_size: usize, max_size: usize },
    /// 无效参数
    InvalidArgument { argument: String },
    /// 数据库版本不匹配
    DatabaseVersionMismatch { expected: u32, actual: u32 },
    /// 事务超时自动回滚
    TransactionTimeout { txn_id: u64 },
    /// 配置错误
    ConfigError { message: String },
    /// 批处理错误
    BatchError { line_number: usize, message: String },
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
            KvError::KeyTooLarge { key_size, max_size } => {
                write!(
                    f,
                    "Key too large: key size {} bytes exceeds maximum limit {} bytes",
                    key_size, max_size
                )
            }
            KvError::ValueTooLarge { value_size, max_size } => {
                write!(
                    f,
                    "Value too large: value size {} bytes exceeds maximum limit {} bytes",
                    value_size, max_size
                )
            }
            KvError::InvalidArgument { argument } => {
                write!(f, "Invalid argument: {}", argument)
            }
            KvError::DatabaseVersionMismatch { expected, actual } => {
                write!(
                    f,
                    "Database version mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            KvError::TransactionTimeout { txn_id } => {
                write!(
                    f,
                    "Transaction timeout: transaction {} has been automatically rolled back",
                    txn_id
                )
            }
            KvError::ConfigError { message } => {
                write!(f, "Configuration error: {}", message)
            }
            KvError::BatchError {
                line_number,
                message,
            } => {
                write!(f, "Batch error at line {}: {}", line_number, message)
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
