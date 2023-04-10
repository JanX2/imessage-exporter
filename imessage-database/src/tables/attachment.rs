/*!
 This module represents common (but not all) columns in the `attachment` table.
*/

use rusqlite::{Connection, Error, Error as E, Result, Row, Statement};
use std::path::{Path, PathBuf};

use crate::{
    error::table::TableError,
    tables::{
        messages::Message,
        table::{Diagnostic, Table, ATTACHMENT},
    },
    util::{
        dirs::home,
        output::{done_processing, processing},
    },
};

/// Represents the MIME type of a message's attachment data
#[derive(Debug, PartialEq, Eq)]
pub enum MediaType<'a> {
    Image(&'a str),
    Video(&'a str),
    Audio(&'a str),
    Text(&'a str),
    Application(&'a str),
    Other(&'a str),
    Unknown,
}

/// Represents a single row in the `attachment` table.
#[derive(Debug)]
pub struct Attachment {
    pub rowid: i32,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub transfer_name: Option<String>,
    pub total_bytes: i32,
    pub hide_attachment: i32,
    pub copied_path: Option<PathBuf>,
}

impl Table for Attachment {
    fn from_row(row: &Row) -> Result<Attachment> {
        Ok(Attachment {
            rowid: row.get("rowid")?,
            filename: row.get("filename").unwrap_or(None),
            mime_type: row.get("mime_type").unwrap_or(None),
            transfer_name: row.get("transfer_name").unwrap_or(None),
            total_bytes: row.get("total_bytes").unwrap_or_default(),
            hide_attachment: row.get("hide_attachment").unwrap_or(0),
            copied_path: None,
        })
    }

    fn get(db: &Connection) -> Result<Statement, TableError> {
        db.prepare(&format!("SELECT * from {}", ATTACHMENT))
            .map_err(TableError::Attachment)
    }

    fn extract(attachment: Result<Result<Self, Error>, Error>) -> Result<Self, TableError> {
        match attachment {
            Ok(attachment) => match attachment {
                Ok(att) => Ok(att),
                // TODO: When does this occur?
                Err(why) => Err(TableError::Attachment(why)),
            },
            // TODO: When does this occur?
            Err(why) => Err(TableError::Attachment(why)),
        }
    }
}

impl Diagnostic for Attachment {
    /// Emit diagnostic data for the Attachments table
    ///
    /// Get the number of attachments that are missing from the filesystem
    /// or are missing one of the following columns:
    ///
    /// - ck_server_change_token_blob
    /// - sr_ck_server_change_token_blob
    ///
    /// # Example:
    ///
    /// ```
    /// use imessage_database::util::dirs::default_db_path;
    /// use imessage_database::tables::table::{Diagnostic, get_connection};
    /// use imessage_database::tables::attachment::Attachment;
    ///
    /// let db_path = default_db_path();
    /// let conn = get_connection(&db_path).unwrap();
    /// Attachment::run_diagnostic(&conn);
    /// ```
    fn run_diagnostic(db: &Connection) {
        processing();
        let mut statement_ck = db
            .prepare(&format!(
                "SELECT count(rowid) FROM {ATTACHMENT} WHERE typeof(ck_server_change_token_blob) == 'text'"
            ))
            .unwrap();
        let num_blank_ck: i32 = statement_ck.query_row([], |r| r.get(0)).unwrap_or(0);

        let mut statement_sr = db
            .prepare(&format!("SELECT filename FROM {ATTACHMENT}"))
            .unwrap();
        let paths = statement_sr.query_map([], |r| Ok(r.get(0))).unwrap();

        let home = home();
        let missing_files = paths
            .filter_map(Result::ok)
            .filter(|path: &Result<String, E>| {
                if let Ok(path) = path {
                    !Path::new(&path.replace('~', &home)).exists()
                } else {
                    false
                }
            })
            .count();

        if num_blank_ck > 0 || missing_files > 0 {
            println!("\rMissing attachment data:");
        } else {
            done_processing();
        }
        if missing_files > 0 {
            println!("    Missing files: {missing_files:?}");
        }
        if num_blank_ck > 0 {
            println!("    ck_server_change_token_blob: {num_blank_ck:?}");
        }
    }
}

impl Attachment {
    /// Gets a Vector of attachments for a single message
    pub fn from_message(db: &Connection, msg: &Message) -> Result<Vec<Attachment>, TableError> {
        let mut out_l = vec![];
        if msg.has_attachments() {
            let mut statement = db
                .prepare(&format!(
                    "
                    SELECT * FROM message_attachment_join j 
                        LEFT JOIN attachment AS a ON j.attachment_id = a.ROWID
                    WHERE j.message_id = {}
                    ",
                    msg.rowid
                ))
                .unwrap();

            let iter = statement
                .query_map([], |row| Ok(Attachment::from_row(row)))
                .unwrap();

            for attachment in iter {
                let m = Attachment::extract(attachment)?;
                out_l.push(m)
            }
        }
        Ok(out_l)
    }

    /// Get the media type of an attachment
    pub fn mime_type(&'_ self) -> MediaType<'_> {
        match &self.mime_type {
            Some(mime) => {
                if let Some(mime_str) = mime.split('/').next() {
                    match mime_str {
                        "image" => MediaType::Image(mime),
                        "video" => MediaType::Video(mime),
                        "audio" => MediaType::Audio(mime),
                        "text" => MediaType::Text(mime),
                        "application" => MediaType::Application(mime),
                        _ => MediaType::Other(mime),
                    }
                } else {
                    MediaType::Other(mime)
                }
            }
            None => MediaType::Unknown,
        }
    }

    /// Get the path to an attachment, if it exists
    pub fn path(&self) -> Option<&Path> {
        match &self.filename {
            Some(name) => Some(Path::new(name)),
            None => None,
        }
    }

    /// Get the extension of an attachment, if it exists
    pub fn extension(&self) -> Option<&str> {
        match self.path() {
            Some(path) => match path.extension() {
                Some(ext) => ext.to_str(),
                None => None,
            },
            None => None,
        }
    }

    /// Get a reasonable filename for an attachment
    pub fn filename(&self) -> &str {
        if let Some(transfer_name) = &self.transfer_name {
            return transfer_name;
        }
        if let Some(filename) = &self.filename {
            return filename;
        }
        "Attachment missing name metadata!"
    }
}

#[cfg(test)]
mod tests {
    use crate::tables::attachment::{Attachment, MediaType};

    use std::path::Path;

    fn sample_attachment() -> Attachment {
        Attachment {
            rowid: 1,
            filename: Some("a/b/c.png".to_string()),
            mime_type: Some("image".to_string()),
            transfer_name: Some("c.png".to_string()),
            total_bytes: 100,
            hide_attachment: 0,
            copied_path: None,
        }
    }

    #[test]
    fn can_get_path() {
        let attachment = sample_attachment();
        assert_eq!(attachment.path(), Some(Path::new("a/b/c.png")));
    }

    #[test]
    fn cant_get_path_missing() {
        let mut attachment = sample_attachment();
        attachment.filename = None;
        assert_eq!(attachment.path(), None);
    }

    #[test]
    fn can_get_extension() {
        let attachment = sample_attachment();
        assert_eq!(attachment.extension(), Some("png"));
    }

    #[test]
    fn cant_get_extension_missing() {
        let mut attachment = sample_attachment();
        attachment.filename = None;
        assert_eq!(attachment.extension(), None);
    }

    #[test]
    fn can_get_mime_type() {
        let attachment = sample_attachment();
        assert_eq!(attachment.mime_type(), MediaType::Image("image"));
    }

    #[test]
    fn can_get_mime_type_fake() {
        let mut attachment = sample_attachment();
        attachment.mime_type = Some("bloop".to_string());
        assert_eq!(attachment.mime_type(), MediaType::Other("bloop"));
    }

    #[test]
    fn can_get_mime_type_missing() {
        let mut attachment = sample_attachment();
        attachment.mime_type = None;
        assert_eq!(attachment.mime_type(), MediaType::Unknown);
    }

    #[test]
    fn can_get_filename() {
        let attachment = sample_attachment();
        assert_eq!(attachment.filename(), "c.png");
    }

    #[test]
    fn can_get_filename_no_transfer_name() {
        let mut attachment = sample_attachment();
        attachment.transfer_name = None;
        assert_eq!(attachment.filename(), "a/b/c.png");
    }

    #[test]
    fn can_get_filename_no_filename() {
        let mut attachment = sample_attachment();
        attachment.filename = None;
        assert_eq!(attachment.filename(), "c.png");
    }

    #[test]
    fn can_get_filename_no_meta() {
        let mut attachment = sample_attachment();
        attachment.transfer_name = None;
        attachment.filename = None;
        assert_eq!(attachment.filename(), "Attachment missing name metadata!");
    }
}
