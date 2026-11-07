//! The resume-token check (#62).

use harpseal::{Delivery, Locale, Message, Metering, ResumeToken};

use super::Target;

/// A resume token the adapter did not write (another adapter's, or a
/// stale one) is ignored: the batch goes out whole, as a keyed batch
/// without a token would, one receipt per Message and one metered unit
/// per metered Push request.
pub async fn a_foreign_resume_token_is_ignored(t: &Target) {
    let kind = t.channel.kind();
    let to = &t.fixtures.to;
    let batch: Vec<Message> = (0..3)
        .map(|i| Message::text(format!("ประกาศมหาวิทยาลัยอ่าวแมวน้ำ {i}")))
        .collect();
    let foreign = ResumeToken::new("another-adapter;7;not-a-digest");
    let out = t
        .channel
        .send_batch_resumed(
            to,
            &Delivery::Push,
            &batch,
            &Locale::th(),
            "contract-resume",
            Some(&foreign),
        )
        .await;
    assert!(
        out.resume.is_none(),
        "{kind}: a complete batch has no token"
    );
    let receipts = out
        .into_result()
        .unwrap_or_else(|e| panic!("{kind}: a batch with a foreign token failed: {e}"));
    assert_eq!(receipts.len(), 3, "{kind}: one receipt per Message");
    let log = t.wire.take();
    let objects: usize = log.calls.iter().map(|c| c.objects).sum();
    assert!(
        objects >= 3,
        "{kind}: every Message went out, got {objects} objects"
    );
    let per_push = u32::from(t.wire.metering(&Delivery::Push) == Metering::Metered);
    let units: u32 = receipts.iter().map(|r| r.metered_units).sum();
    assert_eq!(
        units as usize,
        log.calls.len() * per_push as usize,
        "{kind}: one metered unit per Push request"
    );
}
