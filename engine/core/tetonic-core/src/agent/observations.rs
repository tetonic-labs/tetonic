//! Size a batch of pageable observations without erasing retained evidence.
const BATCH_BYTES: usize = 6144;
const PAGE_ENVELOPE: usize = 512;
const MIN_PAGE_BYTES: usize = 512;

pub(super) fn page_bytes(pages: usize) -> usize {
    if pages == 0 {
        return 0;
    }
    // A stable size avoids progressively smaller pages that force extra model
    // calls to reconstruct the same source. Admission remains the ledger's job.
    // Reserve approximate framing, with a source-body floor for useful reads.
    BATCH_BYTES
        .saturating_sub(PAGE_ENVELOPE.saturating_mul(pages))
        .saturating_div(pages)
        .max(MIN_PAGE_BYTES.min(BATCH_BYTES / pages))
        .max(4)
}
