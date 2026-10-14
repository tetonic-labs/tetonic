//! Size a batch of pageable observations without erasing retained evidence.
const BATCH_BYTES: usize = 24 * 1024;
const PAGE_ENVELOPE: usize = 512;
const MIN_PAGE_BYTES: usize = 1024;

pub(super) fn page_bytes(pages: usize, context_tokens: usize) -> usize {
    if pages == 0 {
        return 0;
    }
    // Six parallel reads still receive useful source pages (3.5 KiB each).
    // A stable size avoids progressively smaller pages that force extra model
    // calls to reconstruct the same source. Admission remains the ledger's job.
    // Reserve approximate framing, with a source-body floor for useful reads.
    // At most half of a small context window goes to this observation batch.
    // This does not shrink as cumulative usage rises (which causes tiny-page loops).
    let batch = BATCH_BYTES.min(context_tokens.saturating_mul(3) / 2);
    batch
        .saturating_sub(PAGE_ENVELOPE.saturating_mul(pages))
        .saturating_div(pages)
        .max(MIN_PAGE_BYTES.min(batch / pages))
        .clamp(4, 4096)
}
