use kv_engine::{KVEngine, Repl};

fn main() {
    let db_path = "data/kvstore.db";

    // 确保数据目录存在
    std::fs::create_dir_all("data").ok();

    // 打开数据库
    match KVEngine::open(db_path) {
        Ok(engine) => {
            let mut repl = Repl::new(engine);
            repl.run();
        }
        Err(e) => {
            eprintln!("打开数据库失败: {}", e);
            std::process::exit(1);
        }
    }
}
