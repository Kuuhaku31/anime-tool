

pub struct PostgresConfig {
    host: String,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    database: Option<String>,
}

impl PostgresConfig {

    pub fn new_from_env() -> Self {
        Self {
            host: std::env::var("POSTGRESQL_URL").expect("POSTGRESQL_URL not set"),
            port: std::env::var("POSTGRESQL_PORT").ok().map(|s| s.parse().expect("POSTGRESQL_PORT must be a number")),
            username: std::env::var("POSTGRESQL_USERNAME").ok(),
            password: std::env::var("POSTGRESQL_PASSWORD").ok(),
            database: std::env::var("POSTGRESQL_DATABASE").ok(),
        }
    }

    pub fn host(&self) -> &str { &self.host }
    pub fn port(&self) -> Option<u16> { self.port }
    pub fn username(&self) -> Option<&str> { self.username.as_deref() }
    pub fn password(&self) -> Option<&str> { self.password.as_deref() }
    pub fn database(&self) -> Option<&str> { self.database.as_deref() }

    pub fn print_self(&self) {
        println!("PostgresConfig:");
        println!("    host:     {}", self.host);
        println!("    port:     {}", self.port.map(|p| p.to_string()).unwrap_or("未指定".to_string()));
        println!("    username: {}", self.username.as_deref().unwrap_or("<None>"));
        println!("    password: {}", if self.password.is_some() { "****" } else { "<None>" });
        println!("    database: {}", self.database.as_deref().unwrap_or("<None>"));
    }

    pub fn create_connection(&self) -> Result<postgres::Client, String> {

        let mut config = postgres::Config::new();
        config.host(&self.host);
        if let Some(port) = self.port {
            config.port(port);
        }
        if let Some(ref username) = self.username {
            config.user(username);
        }
        if let Some(ref password) = self.password {
            config.password(password);
        }
        if let Some(ref database) = self.database {
            config.dbname(database);
        }
        Ok(config.connect(postgres::NoTls).map_err(|e| format!("创建数据库连接失败: {e}"))?)
    }
}
