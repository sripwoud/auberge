#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub struct RsyncProgress {
    pub bytes_transferred: u64,
    pub percent: u8,
    pub speed: String,
    pub eta: String,
}

#[allow(dead_code)]
pub fn parse_rsync_progress(line: &str) -> Option<RsyncProgress> {
    let line = line.trim_end_matches('\r');
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let percent_str = fields[1].strip_suffix('%')?;
    let percent: u8 = percent_str.parse().ok()?;
    let bytes_str = fields[0].replace(',', "");
    let bytes_transferred: u64 = bytes_str.parse().ok()?;
    Some(RsyncProgress {
        bytes_transferred,
        percent,
        speed: fields[2].to_string(),
        eta: fields[3].to_string(),
    })
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
                bytes_transferred: 1234567,
                percent: 42,
                speed: "12.34MB/s".to_string(),
                eta: "0:01:23".to_string(),
            }
        );
    }

    #[test]
    fn parse_rsync_single_digit_percent() {
        let line = "  500  5%   1.00MB/s    0:00:01";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.percent, 5);
        assert_eq!(p.bytes_transferred, 500);
    }

    #[test]
    fn parse_rsync_100_percent() {
        let line = "  10,000,000 100%   50.00MB/s    0:00:00";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.percent, 100);
    }

    #[test]
    fn parse_rsync_plain_text_returns_none() {
        assert!(parse_rsync_progress("sending incremental file list").is_none());
    }

    #[test]
    fn parse_rsync_too_few_fields_returns_none() {
        assert!(parse_rsync_progress("1234 42%").is_none());
    }

    #[test]
    fn parse_rsync_strips_trailing_carriage_return() {
        let line = "  1,234,567  42%   12.34MB/s    0:01:23\r";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.eta, "0:01:23");
        assert_eq!(p.percent, 42);
    }
}
