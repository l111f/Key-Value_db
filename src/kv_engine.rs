use crate::btree::BPlusTree;
use crate::buffer_pool::BufferPoolManager;
use crate::disk_manager::DiskManager;
use crate::error::{KvError, Result};
use crate::recovery::RecoveryManager;
use crate::transaction::TransactionManager;
use crate::wal::{WALManager, WALOpType};

/// KV 存储引擎核心接口
pub struct KVEngine {
    /// 事务管理器（持有 BPlusTree -> BufferPool -> DiskManager 链）
    txn_manager: Option<TransactionManager>,
    /// 数据库是否打开
    is_open: bool,
}

impl KVEngine {
    /// 打开数据库
    /// 按依赖顺序初始化子系统：
    /// DiskManager → BufferPoolManager → BPlusTree → WALManager → TransactionManager
    /// 构造 RecoveryManager 执行崩溃恢复
    pub fn open(db_path: &str) -> Result<KVEngine> {
        // 1. DiskManager
        let disk_manager = DiskManager::new(db_path)?;

        // 2. BufferPoolManager
        let buffer_pool = BufferPoolManager::new(1000, disk_manager);

        // 3. BPlusTree
        let btree = BPlusTree::new(64, buffer_pool);

        // 4. WALManager - 需要单独的 DiskManager 来读取 WAL
        //    这里 WALManager 需要通过 DiskManager 访问 WAL 文件
        //    由于 DiskManager 同时管理 .db 和 .wal 文件，
        //    WALManager 应该持有自己的 DiskManager（或直接操作文件）
        //    但当前架构中 WALManager 持有 DiskManager，
        //    而 BufferPoolManager 也持有 DiskManager，
        //    存在所有权冲突。
        //
        //    解决方案：让 WALManager 直接操作文件，不通过 DiskManager
        //    或者在 open 阶段将 DiskManager 的 WAL 文件句柄分离
        //
        //    简化实现：使用另一个 DiskManager 实例来管理 WAL
        let wal_path = db_path.replace(".db", "_wal.db");
        let wal_disk_manager = DiskManager::new(&wal_path)?;
        let wal_manager = WALManager::new(wal_disk_manager)?;

        // 5. 执行崩溃恢复
        let mut recovery = RecoveryManager::new(wal_manager, btree);
        let stats = recovery.recover_from_wal()?;
        if stats.redo_count > 0 {
            eprintln!(
                "Recovery: {} records replayed, {} skipped",
                stats.redo_count, stats.skip_count
            );
        }
        let (wal_manager, btree) = recovery.into_components();

        // 6. TransactionManager
        let txn_manager = TransactionManager::new(wal_manager, btree);

        Ok(KVEngine {
            txn_manager: Some(txn_manager),
            is_open: true,
        })
    }

    /// 检查数据库是否打开
    fn check_open(&self) -> Result<()> {
        if self.is_open {
            Ok(())
        } else {
            Err(KvError::DatabaseClosed)
        }
    }

    /// 插入键值对
    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;

        if txn_mgr.in_transaction() {
            txn_mgr.record_operation(WALOpType::Put, key, Some(value))?;
        } else {
            txn_mgr.get_btree_mut().insert(key, value)?;
        }
        Ok(())
    }

    /// 获取键对应的值
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;

        // 若在事务中先检查未提交修改
        if txn_mgr.in_transaction() {
            if let Some(uncommitted) = txn_mgr.get_uncommitted(key) {
                return Ok(uncommitted);
            }
        }

        // 否则从 B+树查找
        Ok(txn_mgr.get_btree_mut().search(key))
    }

    /// 删除键
    pub fn delete(&mut self, key: &[u8]) -> Result<bool> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;

        if txn_mgr.in_transaction() {
            // 检查 key 是否存在
            let exists = txn_mgr.get_uncommitted(key)
                .map(|v| v.is_some())
                .unwrap_or_else(|| txn_mgr.get_btree_mut().search(key).is_some());

            if exists {
                txn_mgr.record_operation(WALOpType::Delete, key, None)?;
                return Ok(true);
            }
            Ok(false)
        } else {
            Ok(txn_mgr.get_btree_mut().remove(key)?)
        }
    }

    /// 范围扫描
    pub fn scan(&mut self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;
        Ok(txn_mgr.get_btree_mut().range_scan(start, end))
    }

    /// 开启事务
    pub fn begin(&mut self) -> Result<u64> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;
        txn_mgr.begin()
    }

    /// 提交当前事务
    pub fn commit(&mut self) -> Result<()> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;
        txn_mgr.commit()?;
        // 提交后刷写脏页
        txn_mgr.flush_all()?;
        Ok(())
    }

    /// 回滚当前事务
    pub fn rollback(&mut self) -> Result<()> {
        self.check_open()?;
        let txn_mgr = self.txn_manager.as_mut().ok_or(KvError::DatabaseClosed)?;
        txn_mgr.rollback()
    }

    /// 查询是否在事务中
    pub fn in_transaction(&self) -> bool {
        self.txn_manager
            .as_ref()
            .map(|m| m.in_transaction())
            .unwrap_or(false)
    }

    /// 关闭数据库
    pub fn close(&mut self) -> Result<()> {
        if !self.is_open {
            return Ok(());
        }

        // 刷写所有脏页
        if let Some(txn_mgr) = self.txn_manager.as_mut() {
            txn_mgr.flush_all()?;
            // 清空 WAL
            txn_mgr.get_wal_manager_mut().clear()?;
        }

        self.is_open = false;
        Ok(())
    }

    /// 检查是否打开
    pub fn is_open(&self) -> bool {
        self.is_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    fn create_test_engine() -> (KVEngine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let engine = KVEngine::open(&db_path).unwrap();
        (engine, dir)
    }

    #[test]
    fn test_open_close() {
        let (mut engine, _dir) = create_test_engine();
        assert!(engine.is_open());
        engine.close().unwrap();
        assert!(!engine.is_open());
    }

    #[test]
    fn test_put_get_delete() {
        let (mut engine, _dir) = create_test_engine();

        // Put
        engine.put(b"key1", b"value1").unwrap();
        engine.put(b"key2", b"value2").unwrap();

        // Get
        assert_eq!(engine.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(engine.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(engine.get(b"key3").unwrap(), None);

        // Delete
        assert_eq!(engine.delete(b"key1").unwrap(), true);
        assert_eq!(engine.get(b"key1").unwrap(), None);
        assert_eq!(engine.delete(b"key1").unwrap(), false);

        engine.close().unwrap();
    }

    #[test]
    fn test_put_overwrite() {
        let (mut engine, _dir) = create_test_engine();

        engine.put(b"key1", b"value1").unwrap();
        assert_eq!(engine.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        engine.put(b"key1", b"value2").unwrap();
        assert_eq!(engine.get(b"key1").unwrap(), Some(b"value2".to_vec()));

        engine.close().unwrap();
    }

    #[test]
    fn test_delete_and_reput() {
        let (mut engine, _dir) = create_test_engine();

        engine.put(b"key1", b"value1").unwrap();
        engine.delete(b"key1").unwrap();
        assert_eq!(engine.get(b"key1").unwrap(), None);

        engine.put(b"key1", b"value2").unwrap();
        assert_eq!(engine.get(b"key1").unwrap(), Some(b"value2".to_vec()));

        engine.close().unwrap();
    }

    #[test]
    fn test_scan() {
        let (mut engine, _dir) = create_test_engine();

        for i in 0..10 {
            let key = format!("key{:02}", i);
            let value = format!("value{:02}", i);
            engine.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        let results = engine.scan(b"key03", b"key07").unwrap();
        assert_eq!(results.len(), 5); // key03 ~ key07

        engine.close().unwrap();
    }

    #[test]
    fn test_scan_empty_db() {
        let (mut engine, _dir) = create_test_engine();
        let results = engine.scan(b"a", b"z").unwrap();
        assert!(results.is_empty());
        engine.close().unwrap();
    }

    #[test]
    fn test_scan_no_match() {
        let (mut engine, _dir) = create_test_engine();
        engine.put(b"key1", b"value1").unwrap();
        let results = engine.scan(b"aaa", b"bbb").unwrap();
        assert!(results.is_empty());
        engine.close().unwrap();
    }

    #[test]
    fn test_transaction_commit() {
        let (mut engine, _dir) = create_test_engine();

        engine.begin().unwrap();
        engine.put(b"txn_key1", b"txn_value1").unwrap();
        engine.put(b"txn_key2", b"txn_value2").unwrap();
        engine.commit().unwrap();

        assert_eq!(engine.get(b"txn_key1").unwrap(), Some(b"txn_value1".to_vec()));
        assert_eq!(engine.get(b"txn_key2").unwrap(), Some(b"txn_value2".to_vec()));

        engine.close().unwrap();
    }

    #[test]
    fn test_transaction_rollback() {
        let (mut engine, _dir) = create_test_engine();

        engine.put(b"existing_key", b"existing_value").unwrap();

        engine.begin().unwrap();
        engine.put(b"existing_key", b"modified_value").unwrap();
        engine.put(b"new_key", b"new_value").unwrap();
        engine.rollback().unwrap();

        assert_eq!(engine.get(b"existing_key").unwrap(), Some(b"existing_value".to_vec()));
        assert_eq!(engine.get(b"new_key").unwrap(), None);

        engine.close().unwrap();
    }

    #[test]
    fn test_nested_transaction_error() {
        let (mut engine, _dir) = create_test_engine();

        engine.begin().unwrap();
        let result = engine.begin();
        assert!(result.is_err());
        engine.rollback().unwrap();

        engine.close().unwrap();
    }

    #[test]
    fn test_commit_without_begin_error() {
        let (mut engine, _dir) = create_test_engine();
        let result = engine.commit();
        assert!(result.is_err());
        engine.close().unwrap();
    }

    #[test]
    fn test_rollback_without_begin_error() {
        let (mut engine, _dir) = create_test_engine();
        let result = engine.rollback();
        assert!(result.is_err());
        engine.close().unwrap();
    }

    #[test]
    fn test_closed_database_operations() {
        let (mut engine, _dir) = create_test_engine();
        engine.close().unwrap();

        assert!(engine.put(b"key", b"value").is_err());
        assert!(engine.get(b"key").is_err());
        assert!(engine.delete(b"key").is_err());
        assert!(engine.scan(b"a", b"z").is_err());
    }

    #[test]
    fn test_transaction_read_uncommitted() {
        let (mut engine, _dir) = create_test_engine();

        engine.begin().unwrap();
        engine.put(b"key1", b"value1").unwrap();
        assert_eq!(engine.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        engine.commit().unwrap();

        engine.close().unwrap();
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();

        // 第一次打开并写入
        {
            let mut engine = KVEngine::open(&db_path).unwrap();
            engine.put(b"persistent_key", b"persistent_value").unwrap();
            engine.close().unwrap();
        }

        // 第二次打开并读取
        {
            let mut engine = KVEngine::open(&db_path).unwrap();
            assert_eq!(engine.get(b"persistent_key").unwrap(), Some(b"persistent_value".to_vec()));
            engine.close().unwrap();
        }
    }
}
