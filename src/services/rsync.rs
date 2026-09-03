/// The one number a `--info=progress2` line carries that a caller can use.
///
/// The percentage beside it is not a completion signal: rsync divides bytes
/// sent by the size of every file in the list, so an incremental sync finishes
/// at whatever fraction the changed files happen to represent. Completion comes
/// from a `--stats` total instead — see `parse_transferred_size`. It is still
/// parsed and dropped, as the guard that tells a progress line from anything
/// else on the stream.
#[derive(Debug, PartialEq)]
pub struct RsyncProgress {
    pub bytes_transferred: u64,
}

pub fn parse_rsync_progress(line: &str) -> Option<RsyncProgress> {
    let line = line.trim_end_matches('\r');
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    fields[1].strip_suffix('%')?.parse::<u8>().ok()?;
    let bytes_transferred: u64 = fields[0].replace(',', "").parse().ok()?;
    Some(RsyncProgress { bytes_transferred })
}

const TRANSFERRED_SIZE_PREFIX: &str = "Total transferred file size:";

/// Reads the byte count `rsync --stats` says it will send.
///
/// Deliberately not `Total file size:`, which counts the whole tree and is the
/// denominator that makes `--info=progress2` percentages useless here.
///
/// Splits on `\r` as well as `\n`: the scan pass carries the same progress2
/// flags as the transfer, so its output contains redraws too.
pub fn parse_transferred_size(stats: &str) -> Option<u64> {
    stats
        .split(['\r', '\n'])
        .find_map(|line| line.trim_start().strip_prefix(TRANSFERRED_SIZE_PREFIX))?
        .split_whitespace()
        .next()?
        .replace(',', "")
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rsync_canonical_line() {
        let line = "    1,234,567  42%   12.34MB/s    0:01:23";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(
            p,
            RsyncProgress {
                bytes_transferred: 1234567
            }
        );
    }

    #[test]
    fn parse_rsync_single_digit_percent() {
        let line = "  500  5%   1.00MB/s    0:00:01";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.bytes_transferred, 500);
    }

    #[test]
    fn parse_rsync_100_percent() {
        let line = "  10,000,000 100%   50.00MB/s    0:00:00";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.bytes_transferred, 10_000_000);
    }

    #[test]
    fn parse_rsync_plain_text_returns_none() {
        assert!(parse_rsync_progress("sending incremental file list").is_none());
    }

    /// The dropped percentage is the guard: without it every four-field line
    /// whose first word parses as a number reads as progress.
    #[test]
    fn parse_rsync_non_numeric_percent_returns_none() {
        assert!(parse_rsync_progress("  1,234  xx%  12.34MB/s  0:01:23").is_none());
    }

    #[test]
    fn parse_rsync_too_few_fields_returns_none() {
        assert!(parse_rsync_progress("1234 42%").is_none());
    }

    #[test]
    fn parse_rsync_strips_trailing_carriage_return() {
        let line = "  1,234,567  42%   12.34MB/s    0:01:23\r";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.bytes_transferred, 1_234_567);
    }

    const STATS_BLOCK: &str = "\nNumber of files: 16 (reg: 15, dir: 1)\nTotal file size: 52,428,800 bytes\nTotal transferred file size: 10,485,760 bytes\nLiteral data: 10,485,760 bytes\n";

    #[test]
    fn parse_transferred_size_reads_comma_grouped_digits() {
        assert_eq!(parse_transferred_size(STATS_BLOCK), Some(10_485_760));
    }

    #[test]
    fn parse_transferred_size_ignores_total_file_size() {
        let stats = "Total file size: 52,428,800 bytes\n";
        assert_eq!(parse_transferred_size(stats), None);
    }

    #[test]
    fn parse_transferred_size_reads_zero() {
        let stats = "Total file size: 52,428,800 bytes\nTotal transferred file size: 0 bytes\n";
        assert_eq!(parse_transferred_size(stats), Some(0));
    }

    #[test]
    fn parse_transferred_size_absent_line_returns_none() {
        assert_eq!(
            parse_transferred_size("sending incremental file list\n"),
            None
        );
    }

    #[test]
    fn parse_transferred_size_tolerates_leading_whitespace() {
        let stats = "   Total transferred file size: 4,096 bytes\n";
        assert_eq!(parse_transferred_size(stats), Some(4096));
    }

    #[test]
    fn parse_transferred_size_survives_a_progress_redraw_on_the_same_line() {
        let stats = "     10,485,760 100%  1.00MB/s 0:00:10\rTotal transferred file size: 10,485,760 bytes\n";
        assert_eq!(parse_transferred_size(stats), Some(10_485_760));
    }

    #[test]
    fn parse_transferred_size_rejects_non_numeric_value() {
        assert_eq!(
            parse_transferred_size("Total transferred file size: N/A\n"),
            None
        );
    }
}
