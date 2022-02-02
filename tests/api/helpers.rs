use std::net::TcpListener;

use sqlx::{Connection, Executor, PgConnection, PgPool};
use zero2prod::configuration::{get_configuration, DatabaseSettings};
use zero2prod::email_client::EmailClient;
use zero2prod::telemetry::{get_subscriber, init_subscriber};

use once_cell::sync::Lazy;

static TRACING: Lazy<()> = Lazy::new(|| {
    let default_subscriber_name = "test";
    let default_filter_level = "info";

    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(
            default_subscriber_name,
            default_filter_level,
            std::io::stdout,
        );
        init_subscriber(subscriber);
    } else {
        let subscriber =
            get_subscriber(default_subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub struct TestApp {
    pub address: String,
    pub db_pool: PgPool,
}

pub async fn spawn_app() -> TestApp {
    Lazy::force(&TRACING);
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();

    let mut configuration = get_configuration().expect("Failed to get configuration");
    configuration.database.database_name = uuid::Uuid::new_v4().to_string();
    let db_pool = configure_database(&configuration.database).await;
    let sender_address = configuration
        .email_client
        .sender()
        .expect("Sender email invalid");
    let timeout = configuration.email_client.timeout();
    let email_client = EmailClient::new(
        configuration.email_client.base_url,
        sender_address,
        configuration.email_client.authorization_token,
        timeout,
    );

    let server = zero2prod::startup::run(listener, db_pool.clone(), email_client)
        .expect("Failed to bind to address");
    let _ = tokio::spawn(server);
    let address = format!("http://127.0.0.1:{}", port);

    TestApp { address, db_pool }
}

async fn configure_database(config: &DatabaseSettings) -> PgPool {
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to postgres");
    connection
        .execute(format!(r#"create database "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database");
    let db_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to postgres");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to apply database migrations");
    db_pool
}
