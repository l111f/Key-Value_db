use kv_engine::{KVEngine, Result};

fn main() -> Result<()> {
    println!("=== KV Engine 示例 ===\n");

    let db_path = "data/kvstore.db";

    // 确保数据目录存在
    std::fs::create_dir_all("data").ok();

    // 1. 打开数据库
    println!("1. 打开数据库: {}", db_path);
    let mut engine = KVEngine::open(db_path)?;
    println!("   数据库已打开\n");

    // 2. 批量 Put
    println!("2. 批量写入数据...");
    for i in 0..10 {
        let key = format!("user:{:04}", i);
        let value = format!("{{\"id\": {}, \"name\": \"user_{}\"}}", i, i);
        engine.put(key.as_bytes(), value.as_bytes())?;
    }
    println!("   已写入 10 条记录\n");

    // 3. Get 验证
    println!("3. Get 验证:");
    for i in 0..3 {
        let key = format!("user:{:04}", i);
        let value = engine.get(key.as_bytes())?;
        match value {
            Some(v) => println!("   {} => {}", key, String::from_utf8_lossy(&v)),
            None => println!("   {} => (不存在)", key),
        }
    }
    println!();

    // 4. Scan 范围查询
    println!("4. Scan 范围查询 [user:0003, user:0007]:");
    let results = engine.scan(b"user:0003", b"user:0007")?;
    for (key, value) in &results {
        println!(
            "   {} => {}",
            String::from_utf8_lossy(key),
            String::from_utf8_lossy(value)
        );
    }
    println!("   共 {} 条结果\n", results.len());

    // 5. 开启事务
    println!("5. 开启事务...");
    let txn_id = engine.begin()?;
    println!("   事务 ID: {}\n", txn_id);

    // 6. 事务内 Put
    println!("6. 事务内写入...");
    engine.put(b"txn:key1", b"txn:value1")?;
    engine.put(b"txn:key2", b"txn:value2")?;
    println!("   已写入 txn:key1, txn:key2");

    // 事务内读取验证
    let val = engine.get(b"txn:key1")?;
    println!(
        "   事务内读取 txn:key1 => {}",
        val.map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("(none)".to_string())
    );
    println!();

    // 7. 提交事务
    println!("7. 提交事务...");
    engine.commit()?;
    println!("   事务已提交\n");

    // 8. 验证持久性
    println!("8. 验证事务提交后的数据持久性:");
    let val1 = engine.get(b"txn:key1")?;
    let val2 = engine.get(b"txn:key2")?;
    println!(
        "   txn:key1 => {}",
        val1.map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("(none)".to_string())
    );
    println!(
        "   txn:key2 => {}",
        val2.map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("(none)".to_string())
    );
    println!();

    // 9. Rollback 验证
    println!("9. Rollback 验证...");
    engine.put(b"before_rollback", b"this_should_be_rolled_back")?;
    println!("   写入 before_rollback (非事务)");
    println!("   before_rollback => {:?}",
        engine.get(b"before_rollback")?.map(|v| String::from_utf8_lossy(&v).to_string()));

    let txn_id2 = engine.begin()?;
    println!("\n   开启新事务 ID: {}", txn_id2);
    engine.put(b"before_rollback", b"modified_in_txn")?;
    println!("   事务内修改 before_rollback");
    println!("   事务内读取 => {:?}",
        engine.get(b"before_rollback")?.map(|v| String::from_utf8_lossy(&v).to_string()));

    engine.rollback()?;
    println!("   事务已回滚");
    println!("   回滚后读取 => {:?}\n",
        engine.get(b"before_rollback")?.map(|v| String::from_utf8_lossy(&v).to_string()));

    // 10. Delete
    println!("10. Delete 操作:");
    let deleted = engine.delete(b"txn:key1")?;
    println!("    删除 txn:key1: {}", if deleted { "成功" } else { "不存在" });
    let val = engine.get(b"txn:key1")?;
    println!("    删除后读取 txn:key1 => {:?}", val.map(|v| String::from_utf8_lossy(&v).to_string()));
    println!();

    // 11. 关闭数据库
    println!("11. 关闭数据库...");
    engine.close()?;
    println!("    数据库已关闭\n");

    // 12. 重新打开验证持久性
    println!("12. 重新打开数据库验证持久性...");
    let mut engine2 = KVEngine::open(db_path)?;
    let val = engine2.get(b"user:0000")?;
    println!(
        "    user:0000 => {}",
        val.map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("(none)".to_string())
    );
    let val = engine2.get(b"txn:key2")?;
    println!(
        "    txn:key2 => {}",
        val.map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("(none)".to_string())
    );
    engine2.close()?;
    println!("    持久性验证完成\n");

    println!("=== 示例完成 ===");
    Ok(())
}
