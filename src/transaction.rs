use std::collections::HashMap;

use crate::btree::BPlusTree;
use crate::error::{KvError, Result};
use crate::wal::{WALManager, WALOpType, WALRecord};

/// 事务状态枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxnState {
    Active,
    Committed,
    Rolledback,
}

/// 事务上下文
#[derive(Debug)]
pub struct Transaction {
    /// 事务 ID
    pub txn_id: u64,
    /// 事务状态
    pub state: TxnState,
    /// 事务内已执行的操作列表
    pub operations: Vec<WALRecord>,
    /// Key 旧值快照（Key → Option<Value>，None 表示 Key 不存在）
    pub snapshot: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl Transaction {
    pub fn new(txn_id: u64) -> Self {
        Transaction {
            txn_id,
            state: TxnState::Active,
            operations: Vec::new(),
            snapshot: HashMap::new(),
        }
    }
}

/// 事务管理器
pub struct TransactionManager {
    /// 当前活跃事务
    current_txn: Option<Transaction>,
    /// 下一个事务 ID
    next_txn_id: u64,
    /// WAL 管理器
    wal_manager: WALManager,
    /// B+树索引（用于快照恢复和数据操作）
    btree: BPlusTree,
}

impl TransactionManager {
    /// 创建新的事务管理器
    pub fn new(wal_manager: WALManager, btree: BPlusTree) -> Self {
        TransactionManager {
            current_txn: None,
            next_txn_id: 1,
            wal_manager,
            btree,
        }
    }

    /// 开启事务
    /// 检查是否已有活跃事务，有则返回 NestedTransaction 错误
    pub fn begin(&mut self) -> Result<u64> {
        if self.current_txn.is_some() {
            return Err(KvError::NestedTransaction);
        }

        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.current_txn = Some(Transaction::new(txn_id));
        Ok(txn_id)
    }

    /// 提交当前事务
    pub fn commit(&mut self) -> Result<()> {
        let txn = self.current_txn.take().ok_or(KvError::NoActiveTransaction)?;

        // 构造 Commit 类型 WALRecord
        let commit_record = WALRecord {
            lsn: 0,
            txn_id: txn.txn_id,
            op_type: WALOpType::Commit,
            key: Vec::new(),
            value: None,
            crc: 0,
        };

        // 追加 Commit 记录到 WAL
        self.wal_manager.append(commit_record)?;

        // 刷写脏页
        self.btree.flush_all()?;

        Ok(())
    }

    /// 回滚当前事务
    pub fn rollback(&mut self) -> Result<()> {
        let txn = self.current_txn.take().ok_or(KvError::NoActiveTransaction)?;

        // 遍历快照恢复所有受影响 Key 的旧值
        for (key, old_value) in &txn.snapshot {
            match old_value {
                Some(value) => {
                    // 旧值存在，回写旧值
                    self.btree.insert(key, value)?;
                }
                None => {
                    // 旧值不存在，说明是事务内新增的 Key，需要删除
                    let _ = self.btree.remove(key);
                }
            }
        }

        // 构造 Rollback 类型 WALRecord
        let rollback_record = WALRecord {
            lsn: 0,
            txn_id: txn.txn_id,
            op_type: WALOpType::Rollback,
            key: Vec::new(),
            value: None,
            crc: 0,
        };

        // 追加 Rollback 记录到 WAL
        self.wal_manager.append(rollback_record)?;

        // 刷写脏页
        self.btree.flush_all()?;

        Ok(())
    }

    /// 查询当前是否在事务中
    pub fn in_transaction(&self) -> bool {
        self.current_txn.is_some()
    }

    /// 在事务中记录操作
    /// 在 snapshot 中记录 Key 的原始值（仅首次修改时获取旧值）
    /// 构造 WALRecord 追加到 operations 和 WAL 日志
    pub fn record_operation(
        &mut self,
        op_type: WALOpType,
        key: &[u8],
        value: Option<&[u8]>,
    ) -> Result<()> {
        let txn = self.current_txn.as_mut().ok_or(KvError::NoActiveTransaction)?;

        // 仅在首次修改此 Key 时记录快照
        if !txn.snapshot.contains_key(key) {
            let old_value = self.btree.search(key);
            txn.snapshot.insert(key.to_vec(), old_value);
        }

        // 构造 WALRecord
        let record = WALRecord {
            lsn: 0,
            txn_id: txn.txn_id,
            op_type: op_type.clone(),
            key: key.to_vec(),
            value: value.map(|v| v.to_vec()),
            crc: 0,
        };

        // 追加到操作列表
        txn.operations.push(record.clone());

        // 追加到 WAL 日志
        self.wal_manager.append(record)?;

        // 执行实际操作
        match op_type {
            WALOpType::Put => {
                self.btree.insert(key, value.unwrap())?;
            }
            WALOpType::Delete => {
                let _ = self.btree.remove(key);
            }
            _ => {}
        }

        Ok(())
    }

    /// 在事务内查找未提交的修改
    /// 遍历 operations 查找对指定 Key 的最新操作
    pub fn get_uncommitted(&self, key: &[u8]) -> Option<Option<Vec<u8>>> {
        let txn = self.current_txn.as_ref()?;

        let mut result: Option<Option<Vec<u8>>> = None;
        for record in &txn.operations {
            if record.key == key {
                match record.op_type {
                    WALOpType::Put => {
                        result = Some(record.value.clone());
                    }
                    WALOpType::Delete => {
                        result = Some(None);
                    }
                    _ => {}
                }
            }
        }
        result
    }

    /// 获取 B+树的可变引用
    pub fn get_btree_mut(&mut self) -> &mut BPlusTree {
        &mut self.btree
    }

    /// 获取 WAL 管理器的可变引用
    pub fn get_wal_manager_mut(&mut self) -> &mut WALManager {
        &mut self.wal_manager
    }

    /// 刷写所有脏页
    pub fn flush_all(&mut self) -> Result<()> {
        self.btree.flush_all()
    }

    /// 分配自动事务 ID（用于非事务操作的 WAL 记录）
    /// 不创建 Transaction 对象，仅递增 next_txn_id
    pub fn auto_txn_id(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        id
    }

    /// 写入单条 WAL 记录（用于非事务操作的自动事务）
    pub fn write_wal_record(&mut self, record: WALRecord) -> Result<u64> {
        self.wal_manager.append(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_pool::BufferPoolManager;
    use crate::disk_manager::DiskManager;
    use tempfile;

    fn create_test_txn_manager() -> (tempfile::TempDir, TransactionManager) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = BufferPoolManager::new(100, dm);
        let btree = BPlusTree::new(4, bpm);

        // 需要单独创建 WAL manager
        let dm2 = DiskManager::new(&dir.path().join("test_wal.db").to_str().unwrap().to_string()).unwrap();
        let wal_mgr = WALManager::new(dm2).unwrap();

        let mgr = TransactionManager::new(wal_mgr, btree);
        (dir, mgr)
    }

    #[test]
    fn test_begin_transaction() {
        let (_dir, mut mgr) = create_test_txn_manager();
        let txn_id = mgr.begin().unwrap();
        assert_eq!(txn_id, 1);
        assert!(mgr.in_transaction());
    }

    #[test]
    fn test_nested_transaction_error() {
        let (_dir, mut mgr) = create_test_txn_manager();
        mgr.begin().unwrap();
        let result = mgr.begin();
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_no_transaction_error() {
        let (_dir, mut mgr) = create_test_txn_manager();
        let result = mgr.commit();
        assert!(result.is_err());
    }

    #[test]
    fn test_rollback_no_transaction_error() {
        let (_dir, mut mgr) = create_test_txn_manager();
        let result = mgr.rollback();
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_transaction() {
        let (_dir, mut mgr) = create_test_txn_manager();
        mgr.begin().unwrap();
        let result = mgr.commit();
        assert!(result.is_ok());
        assert!(!mgr.in_transaction());
    }

    #[test]
    fn test_rollback_transaction() {
        let (_dir, mut mgr) = create_test_txn_manager();
        mgr.begin().unwrap();
        let result = mgr.rollback();
        assert!(result.is_ok());
        assert!(!mgr.in_transaction());
    }

    #[test]
    fn test_put_and_get_uncommitted() {
        let (_dir, mut mgr) = create_test_txn_manager();
        mgr.begin().unwrap();

        mgr.record_operation(WALOpType::Put, b"key1", Some(b"value1")).unwrap();
        assert_eq!(mgr.get_uncommitted(b"key1"), Some(Some(b"value1".to_vec())));
        assert_eq!(mgr.get_uncommitted(b"key2"), None);

        mgr.commit().unwrap();
    }

    #[test]
    fn test_rollback_restores_old_value() {
        let (_dir, mut mgr) = create_test_txn_manager();

        // 先在事务外插入数据
        mgr.btree.insert(b"key1", b"original").unwrap();

        // 开启事务并修改
        mgr.begin().unwrap();
        mgr.record_operation(WALOpType::Put, b"key1", Some(b"modified")).unwrap();

        // 回滚
        mgr.rollback().unwrap();

        // 验证旧值恢复
        assert_eq!(mgr.btree.search(b"key1"), Some(b"original".to_vec()));
    }

    #[test]
    fn test_rollback_removes_new_key() {
        let (_dir, mut mgr) = create_test_txn_manager();

        mgr.begin().unwrap();
        mgr.record_operation(WALOpType::Put, b"new_key", Some(b"new_value")).unwrap();

        mgr.rollback().unwrap();

        // 新 key 应被删除
        assert_eq!(mgr.btree.search(b"new_key"), None);
    }
}