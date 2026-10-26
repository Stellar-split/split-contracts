# Issue #1037 — Advanced Dispute Resolution with Arbitration

## Overview

Implements a comprehensive, multi-stage on-chain dispute resolution and arbitration framework (`contracts/split/src/advanced_dispute.rs`) for `SplitContract`. The system supports structured cryptographic evidence submission, conflict-free arbitrator panel selection with fair rotation, reputation-weighted panel voting across binary and proportional rulings, a post-ruling appeal window and appellate override process, and automated on-chain settlement enforcement.

---

## Architecture & Lifecycle Stages

Each `AdvancedDisputeCase` progresses deterministically through six lifecycle stages (`DisputeStage`):

1. **`EvidenceCollection`**: Triggered upon `create_advanced_dispute`. A conflict-free arbitrator panel is immediately assigned and both `claimant` and `respondent` may submit structured `EvidenceRecord` entries up to `evidence_deadline`.
2. **`ArbitrationReview`**: Entered when the first panel arbitrator casts a vote via `cast_panel_vote`.
3. **`DecisionRendered`**: Reached automatically once a majority quorum (`(panel_size / 2) + 1`) of assigned panel arbitrators have voted. Computes the weighted ruling (`ReleaseToRecipient`, `RefundToPayer`, or `SplitBps(bps)`) and opens the appeal window (`appeal_deadline = now + appeal_window_secs`).
4. **`UnderAppeal`**: Entered if either party invokes `file_dispute_appeal` with new evidence before `appeal_deadline`. Blocks premature settlement enforcement.
5. **`Finalized`**: Reached when `resolve_dispute_appeal` either upholds the original panel ruling or overrides it with a final appellate ruling.
6. **`Enforced`**: Reached when `enforce_dispute_decision` executes the on-chain token transfers between the escrowed invoice balance, `respondent` (recipient), and `claimant` (payer).

---

## Key Capabilities

### 1. Fair, Conflict-Free Arbitrator Panel Selection
- Arbitrators are registered via `register_adv_arbitrator(arbitrator, weight)` into a bounded pool (`MAX_ARBITRATOR_POOL = 50`).
- `select_panel` filters out inactive arbitrators as well as both `claimant` and `respondent` so neither party can ever serve on their own dispute panel.
- Uses a persistent rotation cursor (`AdvDisputeKey::RotationCursor`) and tracks `cases_assigned` / `cases_decided` per arbitrator to ensure fair distribution across the pool.

### 2. Cryptographic Evidence Submission
- Both `claimant` and `respondent` can attach up to `MAX_EVIDENCE_ITEMS` (20) items per dispute before `evidence_deadline`.
- Each `EvidenceRecord` records `EvidenceKind` (`ContractAgreement`, `DeliveryProof`, `CommunicationLog`, `PaymentReceipt`, `AuditReport`, `Other`), a 32-byte `content_hash` (`BytesN<32>`), a human-readable description, and ledger timestamp.

### 3. Weighted Panel Voting & Proportional Split Rulings
- Assigned panel members submit `ArbitrationRuling::ReleaseToRecipient`, `ArbitrationRuling::RefundToPayer`, or `ArbitrationRuling::SplitBps(bps)` along with a 32-byte `rationale_hash`.
- When quorum is reached, `tally_weighted_ruling` weights each vote by the arbitrator's profile weight and computes either a decisive binary outcome or a weighted-average basis-point split (`SplitBps(avg_bps)`).

### 4. Appeal Window & Appellate Resolution
- Any ruling enters a mandatory timelock (`appeal_window_secs`, default 24h) before direct enforcement is permitted.
- Either disputing party may file an `AppealRecord` (`file_dispute_appeal`) within the window, transitioning the case to `UnderAppeal`.
- Appellate governance (`resolve_dispute_appeal`) can uphold the panel's original ruling or override it with a revised `ArbitrationRuling`.

### 5. Automated On-Chain Settlement Enforcement
- `enforce_dispute_decision` verifies either that the case is `Finalized` (post-appeal) or that `DecisionRendered` has passed `appeal_deadline` without appeal.
- Computes exact `recipient_payout` and `payer_refund` amounts from `disputed_amount` and transfers tokens directly from the contract escrow to `respondent` and `claimant`, updating the underlying `InvoiceStatus`.

---

## Public Entry Points (`SplitContract`)

| Entry Point | Auth | Description |
|---|---|---|
| `set_adv_dispute_config(config)` | Admin | Configure evidence, voting, and appeal windows + default panel size |
| `get_adv_dispute_config()` | Public | Read active configuration |
| `register_adv_arbitrator(arbitrator, weight)` | Admin | Register or update an arbitrator in the pool |
| `get_adv_arbitrator_pool()` | Public | List all registered arbitrators and workload stats |
| `create_advanced_dispute(...)` | Claimant | Create a dispute and assign a conflict-free panel |
| `submit_dispute_evidence(...)` | Submitter | Attach categorized cryptographic evidence |
| `cast_panel_vote(...)` | Arbitrator | Cast weighted panel vote and auto-tally at quorum |
| `file_dispute_appeal(...)` | Appellant | File an appeal with new evidence hash within window |
| `resolve_dispute_appeal(...)` | Admin | Uphold or override ruling on appeal |
| `enforce_dispute_decision(dispute_id)` | Public | Enforce finalized/unappealed ruling & transfer tokens |
| `get_advanced_dispute(dispute_id)` | Public | Query dispute state |
| `get_dispute_evidence(dispute_id)` | Public | Query all evidence records for a dispute |
| `get_dispute_panel_votes(dispute_id)` | Public | Query all panel votes for a dispute |
| `get_dispute_appeal(dispute_id)` | Public | Query appeal record for a dispute |
