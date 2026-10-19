
use std::env;

use anime_tool::postgres::PostgresConfig;
use anime_tool::qbittorrent::QbConfig;


fn main() -> Result<(), String> {

    // 输出当前工作目录, 用于排查 .env 文件定位问题.
    let cwd = env::current_dir().map_err(|e| format!("获取当前工作目录失败: {e}"))?;
    eprintln!("当前工作目录: {}", cwd.display());

    // 显示 .env 文件的实际加载结果.
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("已加载配置文件: {}", path.display()),
        Err(e) => eprintln!("加载 .env 失败: {e}"),
    }

    // 创建 Postgres 配置
    let postgres_config = PostgresConfig::new_from_env();
    postgres_config.print_self();
    let mut pg_connection = postgres_config.create_connection()?;
    println!("已创建数据库连接配置: {}", postgres_config.host());

    // 创建 qBittorrent 配置
    let qb_config = QbConfig::from_env()?;
    print!("已创建 qBittorrent 配置: ");
    qb_config.print_self();

    let qb_client = qb_config.create_client_and_login()?;
    println!("已创建 qBittorrent 客户端并登录成功:");
    let qb_url = qb_config.url;
    println!("qb_client: {}", qb_url);

    // 获取所有种子 hash_v1
    let mut hash_v1_list = Vec::new();
    let torrent_infos = anime_tool::qbittorrent::get_torrents_info(&qb_client, &qb_url)
        .map_err(|e| format!("获取种子信息失败: {e}"))?;
    println!("获取的种子数量: {}", torrent_infos.len());
    for torrent_info in &torrent_infos {
        hash_v1_list.push(torrent_info.hash_v1);
    }

    /*
    DELETE FROM in_qb WHERE true;
    INSERT INTO in_qb (torrent_hash_v1) VALUES ...
     */
    pg_connection.execute("DELETE FROM in_qb WHERE true", &[]).map_err(|e| format!("清空 in_qb 表失败: {e}"))?;
    let mut transaction = pg_connection.transaction().map_err(|e| format!("创建数据库事务失败: {e}"))?;
    let insert_stmt = transaction.prepare("INSERT INTO in_qb (torrent_hash_v1) VALUES ($1)").map_err(|e| format!("准备插入语句失败: {e}"))?;
    for hash_v1 in &hash_v1_list {
        let hash_v1_str = hex::encode(hash_v1);
        transaction.execute(&insert_stmt, &[&hash_v1_str]).map_err(|e| format!("插入 in_qb 表失败: {e}"))?;
    }
    transaction.commit().map_err(|e| format!("提交事务失败: {e}"))?;

    Ok(())
}