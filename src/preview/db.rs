//! DB preview: SQLite opened strictly read-only with
//! `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` (path flags, never a URI).
//! Lists at most 100 tables, samples at most 3 tables with `LIMIT 5`, caps
//! selected cell bytes in SQL, renders blobs as byte stubs and never issues
//! `COUNT(*)`.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

const TABLE_CAP: usize = 100;
const SAMPLE_TABLES: usize = 3;
const SAMPLE_ROWS: usize = 5;
const SAMPLE_COLS: usize = 24;
const CELL_CAP: usize = 64;

pub fn build(path: &Path) -> Result<crate::preview::DbPreview, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)
        .map_err(|e| format!("not a readable sqlite database: {e}"))?;
    let _ = conn.busy_timeout(Duration::from_millis(250));
    // Belt and braces on top of the read-only open: no statement may mutate.
    let _ = conn.execute_batch("PRAGMA query_only=ON;");

    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|e| format!("cannot read sqlite schema: {e}"))?;
    let mut names: Vec<String> = Vec::new();
    {
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("cannot read sqlite schema: {e}"))?;
        for row in rows {
            match row {
                Ok(name) => names.push(name),
                Err(e) => return Err(format!("cannot read sqlite schema: {e}")),
            }
        }
    }
    drop(stmt);

    let truncated = names.len() > TABLE_CAP;
    names.truncate(TABLE_CAP);

    let mut col_stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1)")
        .map_err(|e| format!("cannot read sqlite columns: {e}"))?;
    let mut tables = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let columns: Option<Vec<String>> = col_stmt
            .query_map([name.as_str()], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.filter_map(Result::ok).collect());
        let rows = match &columns {
            Some(cols) if !cols.is_empty() && i < SAMPLE_TABLES => {
                db_sample_rows(&conn, name, cols)
            }
            _ => None,
        };
        tables.push(crate::preview::DbTable {
            name: name.clone(),
            cols: columns
                .as_ref()
                .map(|c| c.len())
                .unwrap_or(0)
                .min(u32::MAX as usize) as u32,
            columns,
            rows,
        });
    }

    Ok(crate::preview::DbPreview {
        tables,
        truncated,
        note: None,
    })
}

fn db_sample_rows(conn: &Connection, table: &str, columns: &[String]) -> Option<Vec<Vec<String>>> {
    let cols: Vec<&String> = columns.iter().take(SAMPLE_COLS).collect();
    if cols.is_empty() {
        return None;
    }
    let mut sql = String::from("SELECT ");
    for (i, col) in cols.iter().enumerate() {
        if i > 0 {
            sql.push(',');
        }
        let q = quote_ident(col);
        // The CASE keeps a large blob from ever being materialised: only its
        // length crosses the SQLite boundary. Text values are cut to 1 KiB
        // before they reach Rust, then display-capped again below.
        sql.push_str(&format!(
            "typeof({q}) AS t{i}, \
             CASE typeof({q}) WHEN 'blob' THEN length({q}) ELSE NULL END AS l{i}, \
             CASE WHEN typeof({q}) = 'blob' THEN NULL \
                  ELSE substr(CAST({q} AS TEXT), 1, 1024) END AS v{i}"
        ));
    }
    sql.push_str(&format!(" FROM {} LIMIT {SAMPLE_ROWS}", quote_ident(table)));

    let mut stmt = conn.prepare(&sql).ok()?;
    let mut rows = stmt.query([]).ok()?;
    let mut out: Vec<Vec<String>> = Vec::with_capacity(SAMPLE_ROWS);
    while let Some(row) = rows.next().ok()? {
        let mut cells = Vec::with_capacity(cols.len());
        for i in 0..cols.len() {
            let kind: Option<String> = row.get(3 * i).ok();
            let blob_len: Option<i64> = row.get(3 * i + 1).ok();
            let value: Option<String> = row.get(3 * i + 2).ok();
            cells.push(render_cell(kind.as_deref(), blob_len, value));
        }
        out.push(cells);
    }
    Some(out)
}

fn render_cell(kind: Option<&str>, blob_len: Option<i64>, value: Option<String>) -> String {
    match kind {
        Some("null") | None => "null".to_string(),
        Some("blob") => match blob_len {
            Some(n) if n >= 0 => format!("<blob {n} B>"),
            _ => "<blob>".to_string(),
        },
        _ => {
            let text = super::util::scrub_inline(value.as_deref().unwrap_or("null"));
            let (text, _) = super::util::truncate_chars(&text, CELL_CAP);
            text
        }
    }
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
