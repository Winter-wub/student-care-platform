# Notifying chosen Students

How a Module sends a Notice (GLOSSARY) to Students it chose, by chat when
they are linked and by email otherwise, without sending anyone the same
Notice twice. Back to [Building a Deployment](deployment.md).


A Module sends a Notice (GLOSSARY) to Students it chose (a fee reminder, a
grade notice) from a Job or a back-office action with `JobContext::notify`
or `PageState::notify`. It picks the Students itself (no Audience here):

```rust
let notice = Notice::new("reminders", format!("fee_due:w{week}"))
    .locale(Locale::th(), vec![Message::text(th)], "เตือนชำระค่าธรรมเนียม", th)
    .locale(Locale::en(), vec![Message::text(en)], "Fee reminder", en);
let report = cx.notify(&notice, &[NotifyTo::code("68010001")]).await?;
// report.students[i].outcome: Sent | Emailed | Skipped(..) | Deferred(..) | Failed(..) | Overtaken
```

- **Per Student**, in the order given (`NotifyTo::Id` or
  `NotifyTo::code`): the version for the Student's Locale (else the
  default Locale's; else `Skipped(NoText)`) goes as one keyed Push on
  every `active` Link, recorded and metered exactly like a Kernel send
  (`outbound_message` rows with the Module's name and batch key
  `notify:<module>:<key>:<student id>`, `usage_daily`; a refused Push
  marks the Link `unreachable`). With no `active` Link, or when every
  Link turns out unreachable, the Student gets the email (that version's
  subject and plain text) if they have an address, else
  `Skipped(NoAddress)`. An unknown id or code is `Skipped(Unknown)`, a
  Student who `left` is `Skipped(Left)`. `NotifyReport` counts `sent`,
  `emailed`, `skipped`, `deferred`, `failed` and `overtaken`.
- **The key.** The Kernel scopes it by the Module (`<module>:<key>`, so two
  Modules never share one): one key names one Notice, and no Student gets
  it twice, whether the call is retried or runs twice at once (a double
  click). Before it sends to a Student, a call claims that Student's
  `notify_delivery` row (migration 0015) with one conditional write and
  goes on only if the claim is its own; a Student another call is sending
  to is `Deferred(InProgress)`. A claim left by a run that was cut off
  holds for 10 minutes (less than the Job lock), then the next call takes
  it over. A call whose own claim ran out mid-send (it was taken over)
  reports that Student `Overtaken` rather than `Sent` or `Emailed`: the
  call that took over is the one whose report counts. A Student done before is reported again (`again: true`) without
  a send. A new Notice needs a new key (the example names the week). Rows
  are pruned 90 days after their last change, whatever their status (100
  per call, best effort: a failed prune never fails the call). Reusing a
  key after that sends again.
- **Chat after a cut-off run.** On a Channel without retry keys (Telegram,
  the Fake Channel) only the Messages the `outbound_message` rows do not
  record as wholly sent go again, so no Message reaches the Student twice.
  On LINE (`Channel::dedupes_keyed_sends`) the whole batch goes again with
  the same key, so every request gets the same `X-Line-Retry-Key` and LINE
  drops what it already has (ADR-0010; the Job's retries fall well within
  LINE's 24 hours). A chat the rows record whole is never followed by an
  email, even if its Link is gone by the retry.
- **Email after a cut-off run.** The email carries the idempotency key
  `notify:<module>:<key>:<student id>` (`EmailSender::send_keyed`). If a run
  was cut off after it handed an email to the sender, the retry sends it
  again with the same key when the sender de-duplicates
  (`EmailSender::dedupes`: `ResendEmail` sends the `Idempotency-Key`
  header, honoured for 24 hours), and otherwise (SMTP) counts it as sent
  without sending it again: at most once. Keep a key's texts the same:
  Resend refuses a request under a key it saw with another body (409
  `invalid_idempotent_request`, for 24 hours), so an email retried after
  its text changed comes back `Failed(Email)`.
- **Failures.** A rate limit, a partial send or a database error ends the
  call with `Err` (what went out is recorded): let the Job retry with the
  same Notice. A Channel's other refusal is `Failed(Platform)` and a failed
  email `Failed(Email)`; the call goes on with the next Student, and a
  later call with the key tries those again. When a platform refuses Pushes
  because its own quota is used up (LINE's monthly message limit:
  `Error::QuotaExhausted`, also when it stops a batch part-way, as the
  root of a `PartialSend`), its Students get the email instead, that
  Channel is not called again in this call, and it is listed in
  `NotifyReport::exhausted`. A Student who has no address then is
  `Deferred(Quota)`, not skipped: call again once the quota has reset (the
  example's chunk Job re-queues them a day later). That is the platform refusing; the Kernel's
  Quota never stops a send, it only warns Staff.
- **Size and budget.** At most `MAX_NOTIFY` (40) Students per call
  (`Error::Invalid` beyond, nothing sent). A call also keeps to
  `Notice::budget` outgoing requests (default `DEFAULT_BUDGET`, 40, under
  the 50 subrequests of a Workers free-plan invocation; the paid plan
  allows 10,000). Before each Student it adds their worst case: for each
  live Link, one request per Message (a Card List: one per Card; a text:
  one per 4096 characters), plus one email if they have an address. A
  2-Message Notice to a Student with two Links and an email costs 5, so 8
  such Students fill the default. The first Student always goes; once one
  would go over, they and everyone after are `Deferred(Budget)`: call
  again with them. The budget counts outgoing HTTP only, not database
  queries, and covers one call: a tick that runs several Jobs adds theirs
  up. The example's chunk Job hands its deferred Students to a follow-up
  Job and its `Deferred(Quota)` Students to one a day later, and fails the
  run when any `Failed` or `Deferred(InProgress)` remain, so its retry
  tries them.
- See `deployments/example` (`reminders.rs`: the weekly Job and the
  `/admin/reminders` page), `crates/harpseal-testing/tests/module_notify*.rs`
  and `crates/harpseal-channel-line/tests/notify.rs`.
