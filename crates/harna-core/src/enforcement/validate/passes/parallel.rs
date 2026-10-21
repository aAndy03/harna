//! Component `validate-parallel` (1.1.3): one pure function over a list of
//! files, fanned out in ordered chunks. The Rust pass had it inline
//! (0.9.1: 6.4 s single-threaded over 24k files); the web pass ran on one
//! thread until the review of 1.1.2 named it.

/// At least this many files per thread: below it a thread costs more
/// than it saves.
const PER_THREAD: usize = 64;

/// `f` over `items` on as many threads as pay off, results in input order
/// (the same output as one thread).
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(items.len().div_ceil(PER_THREAD).max(1));
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    let f = &f;
    std::thread::scope(|sc| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| sc.spawn(move || c.iter().map(f).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn many_threads_keep_the_order_of_one() {
        let items: Vec<usize> = (0..1000).collect();
        assert_eq!(
            super::map(&items, |i| i * 2),
            items.iter().map(|i| i * 2).collect::<Vec<_>>()
        );
    }
}
