# Issue #1038 — Automated Compliance Reporting

## Overview

Implements an on-chain automated compliance reporting framework (`contracts/split/src/compliance_reporting.rs`) for `SplitContract`. The module provides configurable regulatory report templates, transaction compliance data collection and time-window aggregation, recurring automated report schedules, cryptographic integrity commitments (`SHA-256` over XDR), and multi-format regulatory exports (`Json`, `Csv`, `RegulatoryXml`, `XdrDigest`).

---

## Key Components

### 1. Report Types & Configurable Templates (`ReportTemplate`)
Supports five regulatory reporting categories (`ComplianceReportType`):
- `AmlKycSummary` — AML/KYC flagged activity and volume breakdown
- `TaxWithholding` — Jurisdictional tax withholding totals and fee summaries
- `HighValueTransactions` — Transactions meeting or exceeding a configurable `high_value_threshold`
- `DisputeAndRefundAudit` — Disputed and refunded invoice metrics
- `FullRegulatoryDigest` — Comprehensive multi-metric compliance digest

Each `ReportTemplate` configures:
- `name`: Human-readable template name
- `report_type`: `ComplianceReportType`
- `jurisdiction_filter`: Optional `Symbol` (e.g., `US`, `EU`, `SG`, or `None` for `GLOBAL`)
- `high_value_threshold`: Token amount threshold for high-value transaction flagging
- `include_tax_breakdown` / `include_aml_flags`: Inclusion toggles
- `default_format`: Default `ExportFormat` automatically materialized on report generation

### 2. Compliance Data Collection & Window Aggregation
- `record_compliance_data` records `ComplianceDataPoint` entries (`invoice_id`, `payer`, `amount`, `fee_amount`, `tax_withheld`, `jurisdiction`, `aml_flagged`, `disputed`, `refunded`, `timestamp`) in a bounded rolling ledger buffer (`MAX_COMPLIANCE_RECORDS = 500`).
- `aggregate_compliance_metrics` filters records by `[period_start, period_end]` and optional `jurisdiction_filter`, computing `AggregatedComplianceMetrics` (`total_transactions`, `total_volume`, `total_fees`, `total_tax_withheld`, `high_value_count`, `high_value_volume`, `aml_flagged_count`, `aml_flagged_volume`, `disputed_count`, `refunded_count`).

### 3. Automated Scheduling (`ReportSchedule`)
- `schedule_compliance_report(template_id, interval_secs, start_at)` creates a recurring schedule.
- `run_scheduled_compliance_reports()` evaluates all enabled schedules where `ledger.timestamp() >= next_run_at`, automatically generates the `ComplianceReport` and default export artifact for the elapsed window, updates `last_run_at` and `last_report_id`, and advances `next_run_at`.

### 4. Multi-Format Export & Cryptographic Integrity (`ExportFormat`)
- Every `ComplianceReport` commits a `BytesN<32>` SHA-256 `integrity_hash` over its canonical XDR representation.
- `export_compliance_report(report_id, format)` materializes a `ComplianceExportArtifact` in `Json`, `Csv`, `RegulatoryXml`, or `XdrDigest` format with its own `content_hash`.

---

## Public Entry Points (`SplitContract`)

| Entry Point | Auth | Description |
|---|---|---|
| `register_compliance_template(...)` | Admin | Create a reusable compliance report template |
| `get_compliance_templates()` | Public | List all registered report templates |
| `record_compliance_data(point)` | Admin | Record a transaction compliance data point |
| `generate_compliance_report(template_id, start, end)` | Admin | Generate an on-chain compliance report & default export |
| `export_compliance_report(report_id, format)` | Public | Export an existing report into `Json`, `Csv`, `RegulatoryXml`, or `XdrDigest` |
| `schedule_compliance_report(template_id, interval, start_at)` | Admin | Register an automated recurring report schedule |
| `get_compliance_schedules()` | Public | List all configured report schedules |
| `run_scheduled_compliance_reports()` | Public | Trigger all due scheduled compliance reports |
| `get_compliance_report(report_id)` | Public | Query a generated compliance report by ID |
| `get_compliance_export(report_id, format)` | Public | Query a generated export artifact |
