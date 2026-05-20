pub mod chat;
pub mod clothes;
pub mod feedback;
pub mod health;
pub mod home;
pub mod user;
pub mod outfit;
pub mod recommendation;
pub mod reference;
pub mod region;
pub mod style_mood;
pub mod weather;

use axum::Router;

use crate::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .nest("/health", health::router())
        .nest("/clothes", clothes::router())
        .nest("/region", region::router())
        .nest("/weather", weather::router())
        .nest("/recommendation", recommendation::router())
        .nest("/references", reference::router())
        .nest("/outfit", outfit::router())
        .nest("/chat", chat::router())
        .nest("/feedback", feedback::router())
        .nest("/user", user::router())
        .nest("/style-moods", style_mood::router())
}

pub fn home_router() -> Router<AppState> {
    home::router()
}
