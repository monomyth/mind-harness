//! Native Mind Harness session archive (Parquet).
//!
//! Schema (required): `sample_index`, `exg_0`…`exg_{n-1}`, `accel_x/y/z`, `time`.
//! Optional when those Cyton modes are on: `analog_i`, `digital_i`.
//! File I/O stays on the record thread; this type only encodes bytes.

use crate::data_logger::RecordingSample;
use parquet::basic::Compression;
use parquet::data_type::{DoubleType, Int64Type};
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::file::writer::SerializedFileWriter;
use parquet::record::Field;
use parquet::schema::parser::parse_message_type;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const ROW_GROUP: usize = 512;

pub const COL_SAMPLE_INDEX: &str = "sample_index";
pub const COL_ACCEL_X: &str = "accel_x";
pub const COL_ACCEL_Y: &str = "accel_y";
pub const COL_ACCEL_Z: &str = "accel_z";
pub const COL_TIME: &str = "time";

pub fn exg_col(i: usize) -> String {
    format!("exg_{i}")
}
pub fn analog_col(i: usize) -> String {
    format!("analog_{i}")
}
pub fn digital_col(i: usize) -> String {
    format!("digital_{i}")
}

/// Column names in write order.
pub fn column_names(n_exg: usize, n_analog: usize, n_digital: usize) -> Vec<String> {
    let mut cols = Vec::with_capacity(5 + n_exg + n_analog + n_digital);
    cols.push(COL_SAMPLE_INDEX.to_string());
    for i in 0..n_exg {
        cols.push(exg_col(i));
    }
    cols.push(COL_ACCEL_X.to_string());
    cols.push(COL_ACCEL_Y.to_string());
    cols.push(COL_ACCEL_Z.to_string());
    cols.push(COL_TIME.to_string());
    for i in 0..n_analog {
        cols.push(analog_col(i));
    }
    for i in 0..n_digital {
        cols.push(digital_col(i));
    }
    cols
}

fn message_type(n_exg: usize, n_analog: usize, n_digital: usize) -> String {
    let mut body = String::from("  REQUIRED INT64 sample_index;\n");
    for i in 0..n_exg {
        body.push_str(&format!("  REQUIRED DOUBLE exg_{i};\n"));
    }
    body.push_str("  REQUIRED DOUBLE accel_x;\n");
    body.push_str("  REQUIRED DOUBLE accel_y;\n");
    body.push_str("  REQUIRED DOUBLE accel_z;\n");
    body.push_str("  REQUIRED DOUBLE time;\n");
    for i in 0..n_analog {
        body.push_str(&format!("  REQUIRED DOUBLE analog_{i};\n"));
    }
    for i in 0..n_digital {
        body.push_str(&format!("  REQUIRED DOUBLE digital_{i};\n"));
    }
    format!("message mind_harness {{\n{body}}}")
}

fn pq_err(e: parquet::errors::ParquetError) -> io::Error {
    io::Error::other(e.to_string())
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key.to_string(), Some(value.to_string()))
}

pub struct DataWriterParquet {
    writer: Option<SerializedFileWriter<File>>,
    n_exg: usize,
    n_analog: usize,
    n_digital: usize,
    buf_index: Vec<i64>,
    buf_exg: Vec<Vec<f64>>,
    buf_ax: Vec<f64>,
    buf_ay: Vec<f64>,
    buf_az: Vec<f64>,
    buf_time: Vec<f64>,
    buf_analog: Vec<Vec<f64>>,
    buf_digital: Vec<Vec<f64>>,
    rows_in_buf: usize,
}

impl DataWriterParquet {
    pub fn new(
        path: PathBuf,
        n_exg: usize,
        sample_rate: i32,
        n_analog: usize,
        n_digital: usize,
    ) -> io::Result<Self> {
        let n_exg = n_exg.max(1);
        let schema = Arc::new(parse_message_type(&message_type(n_exg, n_analog, n_digital)).map_err(pq_err)?);
        let props = WriterProperties::builder()
            .set_compression(Compression::UNCOMPRESSED)
            .set_key_value_metadata(Some(vec![
                kv("sample_rate", &sample_rate.to_string()),
                kv("n_exg", &n_exg.to_string()),
                kv("n_analog", &n_analog.to_string()),
                kv("n_digital", &n_digital.to_string()),
                kv("mind_harness", env!("CARGO_PKG_VERSION")),
            ]))
            .build();
        let file = File::create(&path)?;
        let writer = SerializedFileWriter::new(file, schema, Arc::new(props)).map_err(pq_err)?;
        Ok(Self {
            writer: Some(writer),
            n_exg,
            n_analog,
            n_digital,
            buf_index: Vec::with_capacity(ROW_GROUP),
            buf_exg: vec![Vec::with_capacity(ROW_GROUP); n_exg],
            buf_ax: Vec::with_capacity(ROW_GROUP),
            buf_ay: Vec::with_capacity(ROW_GROUP),
            buf_az: Vec::with_capacity(ROW_GROUP),
            buf_time: Vec::with_capacity(ROW_GROUP),
            buf_analog: vec![Vec::with_capacity(ROW_GROUP); n_analog],
            buf_digital: vec![Vec::with_capacity(ROW_GROUP); n_digital],
            rows_in_buf: 0,
        })
    }

    pub fn write_sample(&mut self, rec: &RecordingSample, fallback_time: f64) -> io::Result<()> {
        self.buf_index.push(rec.packet_index.round() as i64);
        for i in 0..self.n_exg {
            self.buf_exg[i].push(rec.exg.get(i).copied().unwrap_or(0.0));
        }
        self.buf_ax.push(rec.accel[0]);
        self.buf_ay.push(rec.accel[1]);
        self.buf_az.push(rec.accel[2]);
        let t = if rec.time.is_finite() && rec.time != 0.0 {
            rec.time
        } else {
            fallback_time
        };
        self.buf_time.push(t);
        for i in 0..self.n_analog {
            self.buf_analog[i].push(rec.analog.get(i).copied().unwrap_or(0.0));
        }
        for i in 0..self.n_digital {
            self.buf_digital[i].push(rec.digital.get(i).copied().unwrap_or(0.0));
        }
        self.rows_in_buf += 1;
        if self.rows_in_buf >= ROW_GROUP {
            self.flush_row_group()?;
        }
        Ok(())
    }

    fn flush_row_group(&mut self) -> io::Result<()> {
        if self.rows_in_buf == 0 {
            return Ok(());
        }
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("parquet writer closed"))?;
        let mut rg = writer.next_row_group().map_err(pq_err)?;

        write_i64(&mut rg, &self.buf_index)?;
        for ch in 0..self.n_exg {
            write_f64(&mut rg, &self.buf_exg[ch])?;
        }
        write_f64(&mut rg, &self.buf_ax)?;
        write_f64(&mut rg, &self.buf_ay)?;
        write_f64(&mut rg, &self.buf_az)?;
        write_f64(&mut rg, &self.buf_time)?;
        for ch in 0..self.n_analog {
            write_f64(&mut rg, &self.buf_analog[ch])?;
        }
        for ch in 0..self.n_digital {
            write_f64(&mut rg, &self.buf_digital[ch])?;
        }
        rg.close().map_err(pq_err)?;
        self.clear_bufs();
        Ok(())
    }

    fn clear_bufs(&mut self) {
        self.buf_index.clear();
        for c in &mut self.buf_exg {
            c.clear();
        }
        self.buf_ax.clear();
        self.buf_ay.clear();
        self.buf_az.clear();
        self.buf_time.clear();
        for c in &mut self.buf_analog {
            c.clear();
        }
        for c in &mut self.buf_digital {
            c.clear();
        }
        self.rows_in_buf = 0;
    }

    pub fn close(&mut self) -> io::Result<()> {
        self.flush_row_group()?;
        if let Some(w) = self.writer.take() {
            w.close().map_err(pq_err)?;
        }
        Ok(())
    }
}

fn write_i64(
    rg: &mut parquet::file::writer::SerializedRowGroupWriter<'_, File>,
    vals: &[i64],
) -> io::Result<()> {
    let mut col = rg
        .next_column()
        .map_err(pq_err)?
        .ok_or_else(|| io::Error::other("parquet: missing INT64 column"))?;
    col.typed::<Int64Type>()
        .write_batch(vals, None, None)
        .map_err(pq_err)?;
    col.close().map_err(pq_err)?;
    Ok(())
}

fn write_f64(
    rg: &mut parquet::file::writer::SerializedRowGroupWriter<'_, File>,
    vals: &[f64],
) -> io::Result<()> {
    let mut col = rg
        .next_column()
        .map_err(pq_err)?
        .ok_or_else(|| io::Error::other("parquet: missing DOUBLE column"))?;
    col.typed::<DoubleType>()
        .write_batch(vals, None, None)
        .map_err(pq_err)?;
    col.close().map_err(pq_err)?;
    Ok(())
}

/// One decoded Parquet take (rows as `RecordingSample`).
#[derive(Clone, Debug)]
pub struct ParquetRecording {
    pub samples: Vec<RecordingSample>,
    pub sample_rate: i32,
    pub n_exg: usize,
    pub n_analog: usize,
    pub n_digital: usize,
    pub columns: Vec<String>,
}

impl ParquetRecording {
    /// Widget/BDF row: EXG, Accel X/Y/Z, analog, digital, packet index.
    pub fn playback_rows(&self) -> Vec<Vec<f64>> {
        self.samples
            .iter()
            .map(|s| s.playback_row(self.n_exg, self.n_analog, self.n_digital))
            .collect()
    }
}

fn meta_i32(kvs: Option<&[KeyValue]>, key: &str, default: i32) -> i32 {
    let Some(kvs) = kvs else {
        return default;
    };
    for kv in kvs {
        if kv.key == key {
            if let Some(v) = kv.value.as_deref() {
                if let Ok(n) = v.trim().parse() {
                    return n;
                }
            }
        }
    }
    default
}

fn field_f64(f: &Field) -> f64 {
    match f {
        Field::Double(v) => *v,
        Field::Float(v) => *v as f64,
        Field::Long(v) => *v as f64,
        Field::Int(v) => *v as f64,
        _ => 0.0,
    }
}

fn field_i64(f: &Field) -> i64 {
    match f {
        Field::Long(v) => *v,
        Field::Int(v) => *v as i64,
        Field::Double(v) => v.round() as i64,
        _ => 0,
    }
}

pub fn read_parquet(path: &Path) -> io::Result<ParquetRecording> {
    let file = File::open(path)?;
    let reader = SerializedFileReader::new(file).map_err(pq_err)?;
    let md = reader.metadata();
    let file_md = md.file_metadata();
    let kvs = file_md.key_value_metadata();
    let sample_rate = meta_i32(kvs.map(|v| v.as_slice()), "sample_rate", 250).max(1);

    let descr = file_md.schema_descr();
    let mut columns = Vec::new();
    for i in 0..descr.num_columns() {
        columns.push(descr.column(i).name().to_string());
    }
    if columns.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "parquet has no columns",
        ));
    }

    let mut n_exg = meta_i32(kvs.map(|v| v.as_slice()), "n_exg", 0).max(0) as usize;
    let mut n_analog = meta_i32(kvs.map(|v| v.as_slice()), "n_analog", 0).max(0) as usize;
    let mut n_digital = meta_i32(kvs.map(|v| v.as_slice()), "n_digital", 0).max(0) as usize;
    if n_exg == 0 {
        n_exg = columns.iter().filter(|c| c.starts_with("exg_")).count().max(1);
    }
    if n_analog == 0 {
        n_analog = columns.iter().filter(|c| c.starts_with("analog_")).count();
    }
    if n_digital == 0 {
        n_digital = columns.iter().filter(|c| c.starts_with("digital_")).count();
    }

    let mut samples = Vec::new();
    let iter = reader.get_row_iter(None).map_err(pq_err)?;
    for row in iter {
        let row = row.map_err(pq_err)?;
        let mut rec = RecordingSample {
            packet_index: 0.0,
            exg: vec![0.0; n_exg],
            accel: [0.0; 3],
            time: 0.0,
            analog: vec![0.0; n_analog],
            digital: vec![0.0; n_digital],
        };
        for (name, field) in row.get_column_iter() {
            if name == COL_SAMPLE_INDEX {
                rec.packet_index = field_i64(field) as f64;
            } else if name == COL_ACCEL_X {
                rec.accel[0] = field_f64(field);
            } else if name == COL_ACCEL_Y {
                rec.accel[1] = field_f64(field);
            } else if name == COL_ACCEL_Z {
                rec.accel[2] = field_f64(field);
            } else if name == COL_TIME {
                rec.time = field_f64(field);
            } else if let Some(rest) = name.strip_prefix("exg_") {
                if let Ok(i) = rest.parse::<usize>() {
                    if i < rec.exg.len() {
                        rec.exg[i] = field_f64(field);
                    }
                }
            } else if let Some(rest) = name.strip_prefix("analog_") {
                if let Ok(i) = rest.parse::<usize>() {
                    if i < rec.analog.len() {
                        rec.analog[i] = field_f64(field);
                    }
                }
            } else if let Some(rest) = name.strip_prefix("digital_") {
                if let Ok(i) = rest.parse::<usize>() {
                    if i < rec.digital.len() {
                        rec.digital[i] = field_f64(field);
                    }
                }
            }
        }
        samples.push(rec);
    }
    if samples.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "parquet contained no rows",
        ));
    }
    Ok(ParquetRecording {
        samples,
        sample_rate,
        n_exg,
        n_analog,
        n_digital,
        columns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_logger::RecordingSample;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mh220_{tag}_{}.parquet",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn sample(i: i64) -> RecordingSample {
        RecordingSample {
            packet_index: i as f64,
            exg: vec![
                i as f64,
                1.0,
                2.0,
                3.0,
                4.0,
                5.0,
                6.0,
                7.0,
            ],
            accel: [0.01 * i as f64, -0.02, 0.98],
            time: i as f64 / 250.0,
            analog: vec![],
            digital: vec![],
        }
    }

    #[test]
    fn schema_has_index_eight_exg_accel_xyz_time() {
        let names = column_names(8, 0, 0);
        assert_eq!(
            names,
            vec![
                "sample_index",
                "exg_0",
                "exg_1",
                "exg_2",
                "exg_3",
                "exg_4",
                "exg_5",
                "exg_6",
                "exg_7",
                "accel_x",
                "accel_y",
                "accel_z",
                "time",
            ]
        );
    }

    #[test]
    fn schema_adds_analog_and_digital_when_those_modes_are_on() {
        let names = column_names(8, 3, 5);
        assert!(names.contains(&"analog_0".into()));
        assert!(names.contains(&"analog_2".into()));
        assert!(names.contains(&"digital_0".into()));
        assert!(names.contains(&"digital_4".into()));
        assert_eq!(names.len(), 13 + 3 + 5);
    }

    #[test]
    fn roundtrip_keeps_index_exg_accel_time() {
        let path = temp_path("round");
        let mut w = DataWriterParquet::new(path.clone(), 8, 250, 0, 0).unwrap();
        for i in 0..20 {
            w.write_sample(&sample(i), i as f64 / 250.0).unwrap();
        }
        w.close().unwrap();

        let got = read_parquet(&path).unwrap();
        assert_eq!(got.sample_rate, 250);
        assert_eq!(got.n_exg, 8);
        assert_eq!(got.n_analog, 0);
        assert_eq!(got.n_digital, 0);
        assert_eq!(got.samples.len(), 20);
        assert_eq!(got.columns[0], "sample_index");
        assert!(got.columns.contains(&"time".to_string()));
        assert_eq!(got.samples[10].packet_index, 10.0);
        assert!((got.samples[10].exg[0] - 10.0).abs() < 1e-9);
        assert!((got.samples[10].accel[2] - 0.98).abs() < 1e-9);
        assert!((got.samples[10].time - 10.0 / 250.0).abs() < 1e-12);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn analog_mode_writes_analog_columns() {
        let path = temp_path("analog");
        let mut w = DataWriterParquet::new(path.clone(), 8, 250, 3, 0).unwrap();
        let mut rec = sample(3);
        rec.analog = vec![1.5, 2.5, 3.5];
        w.write_sample(&rec, 0.012).unwrap();
        w.close().unwrap();
        let got = read_parquet(&path).unwrap();
        assert_eq!(got.n_analog, 3);
        assert_eq!(got.samples[0].analog, vec![1.5, 2.5, 3.5]);
        assert!(got.columns.iter().any(|c| c == "analog_0"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn digital_mode_writes_digital_columns() {
        let path = temp_path("digital");
        let mut w = DataWriterParquet::new(path.clone(), 8, 250, 0, 5).unwrap();
        let mut rec = sample(1);
        rec.digital = vec![1.0, 0.0, 1.0, 0.0, 1.0];
        w.write_sample(&rec, 0.004).unwrap();
        w.close().unwrap();
        let got = read_parquet(&path).unwrap();
        assert_eq!(got.n_digital, 5);
        assert_eq!(got.samples[0].digital, vec![1.0, 0.0, 1.0, 0.0, 1.0]);
        let _ = std::fs::remove_file(path);
    }
}
