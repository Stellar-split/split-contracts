# Issue #1000: Invoice factoring (design scaffold)

**Status:** design only. No factoring is implemented.

## Goal

A creator sells an invoice before it is paid, to a buyer, for an immediate payment at a discount.
When the payers settle, the buyer receives the full amount.

## Proposed model

- A factoring offer: invoice ID, advance amount (below the invoice total), and the buyer's address.
- The buyer funds the advance. The creator receives it. The invoice's payee becomes the buyer.
- When the invoice is funded, the escrow pays the buyer, not the creator.

## Constraints

- Changing the payee changes the release path in `invoice-escrow`, so the payee change must be made
  before any release, and only while the invoice is open.
- The advance must be below the total, so the buyer's gain is a discount and not a loss to the payers.

## Open questions

- Whether a factored invoice can still be cancelled by the creator, and who is refunded if it is.
- Whether the buyer must be authorised by the payers.

## Not done

No code, no tests, no payee change.
