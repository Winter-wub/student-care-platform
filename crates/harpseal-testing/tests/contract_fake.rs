//! The Channel contract suite against the Fake Channel in every profile.

use harpseal_channel_fake::FakeProfile;
use harpseal_testing::contract::{fake, run};

#[tokio::test]
async fn fake_full_passes_the_channel_contract() {
    run(|| fake(FakeProfile::Full)).await;
}

#[tokio::test]
async fn fake_line_like_passes_the_channel_contract() {
    run(|| fake(FakeProfile::LineLike)).await;
}

#[tokio::test]
async fn fake_telegram_like_passes_the_channel_contract() {
    run(|| fake(FakeProfile::TelegramLike)).await;
}
