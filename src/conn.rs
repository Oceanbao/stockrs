use sqlx::{sqlite::SqlitePoolOptions, Error, FromRow, SqlitePool};

#[derive(Clone, FromRow, Debug)]
struct User {
    user_id: i32,
    username: String,
    email: String,
    password_hash: String,
}

pub async fn get_database_pool(filename: &str) -> Result<SqlitePool, Error> {
    let pool = SqlitePoolOptions::new().connect(filename).await?;

    let migration_result = sqlx::migrate!().run(&pool).await;

    match migration_result {
        Ok(_) => tracing::debug!("Migration success: {:?}", migration_result),
        Err(error) => {
            panic!("error: {}", error);
        }
    }

    let result = sqlx::query!(
        "SELECT name
         FROM sqlite_schema
         WHERE type ='table' 
         AND name NOT LIKE 'sqlite_%';",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    for (idx, row) in result.iter().enumerate() {
        tracing::debug!("[{}]: {:?}", idx, row.name);
    }

    let result = sqlx::query!(
        "INSERT INTO user (user_id, username, email, password_hash) VALUES (?,?,?,?)",
        123,
        "commelamer",
        "commelamer@foo.bar",
        "clajsdf#3i2039",
    )
    .execute(&pool)
    .await
    .unwrap();

    tracing::debug!("Query result: {:?}", result);

    let result = sqlx::query_as!(
        User,
        r#"SELECT user_id as "user_id: i32", username, email, password_hash FROM user"#
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    for user in result {
        tracing::debug!(
            "Query result: [user_id: {:?}]: username: {}, email: {}, password_hash: {}",
            user.user_id,
            &user.username,
            &user.email,
            &user.password_hash
        );
    }

    let result = sqlx::query!("DELETE FROM user WHERE username=?", "commelamer")
        .execute(&pool)
        .await
        .unwrap();

    tracing::debug!("Delete result: {:?}", result);

    Ok(pool)
}
