use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ResticStatus {
    pub percent_done: f64,
    pub total_bytes: Option<u64>,
    pub bytes_done: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ResticSummary {
    pub snapshot_id: String,
    #[allow(dead_code)]
    pub files_new: u64,
    #[allow(dead_code)]
    pub files_changed: u64,
    #[allow(dead_code)]
    pub data_added: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "message_type", rename_all = "lowercase")]
pub enum ResticMessage {
    Status(ResticStatus),
    Summary(ResticSummary),
}

pub fn parse_restic_message(line: &str) -> Option<ResticMessage> {
    serde_json::from_str(line).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_restic_status_line() {
        let line = r#"{"message_type":"status","percent_done":0.5,"total_bytes":1048576,"bytes_done":524288}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done - 0.5).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, Some(1048576));
                assert_eq!(s.bytes_done, Some(524288));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_summary_line() {
        let line = r#"{"message_type":"summary","snapshot_id":"abc123","files_new":10,"files_changed":2,"data_added":1048576}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Summary(s) => {
                assert_eq!(s.snapshot_id, "abc123");
                assert_eq!(s.files_new, 10);
                assert_eq!(s.files_changed, 2);
                assert_eq!(s.data_added, 1048576);
            }
            _ => panic!("expected Summary"),
        }
    }

    #[test]
    fn parse_restic_plain_text_returns_none() {
        assert!(parse_restic_message("using parent snapshot abc123").is_none());
    }

    #[test]
    fn parse_restic_malformed_json_returns_none() {
        assert!(parse_restic_message("{bad json}").is_none());
    }

    #[test]
    fn parse_restic_zero_percent() {
        let line = r#"{"message_type":"status","percent_done":0.0}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, None);
                assert_eq!(s.bytes_done, None);
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_full_percent() {
        let line =
            r#"{"message_type":"status","percent_done":1.0,"total_bytes":100,"bytes_done":100}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => assert!((s.percent_done - 1.0).abs() < f64::EPSILON),
            _ => panic!("expected Status"),
        }
    }
}
