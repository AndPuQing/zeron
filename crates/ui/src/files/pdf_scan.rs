//! A bounded check of an untrusted PDF before hayro parses or renders it.
//! hayro decodes streams and sizes image buffers from what the file declares,
//! with no limits, so a small file can ask for gigabytes. Every stream object
//! is found by its `N G obj` header, before parsing (so the compressed object
//! and cross-reference streams the parser reads are covered too) and again
//! through the parsed document, once references resolve and encrypted data
//! decrypts. Filter chains that multiply expansion are refused, compressed
//! streams are decoded without keeping the output to measure them, and image
//! dictionaries are held to a size budget.
use std::borrow::Cow;
use std::io::Read;

// The reader modules are public for hayro's own crates; the lockfile pins
// the version they come from.
use hayro::hayro_syntax::bit_reader::BitReader;
use hayro::hayro_syntax::object::{Array, Dict, Name, ObjectIdentifier, Stream};
use hayro::hayro_syntax::reader::{Reader, ReaderContext, ReaderExt};
use hayro::hayro_syntax::{Filter, Pdf};

/// Bytes one stream may decode to.
const MAX_STREAM_DECODED: u64 = 256 * 1024 * 1024;
/// Bytes all of a document's streams may decode to together.
const MAX_DOCUMENT_DECODED: u64 = 1024 * 1024 * 1024;
/// Pixels one image may declare.
const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
/// Decoded sample bytes one image may declare.
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

const CHAINED: &str = "This PDF stacks compression filters, which the preview refuses.";
const EXPANDS: &str = "This PDF expands past the preview's memory limit.";
const IMAGE: &str = "This PDF has an image larger than the preview allows.";
const LENGTH: &str = "This PDF declares a stream longer than the file.";

/// What the scan before parsing found, to finish once the document parses.
pub(super) struct Scan {
    file_len: usize,
    /// Whether the file may be encrypted, so its streams are measured again
    /// once hayro decrypts them.
    encrypted: bool,
    streams: Vec<ObjectIdentifier>,
    /// Streams whose filter/length values were indirect and need the parsed
    /// cross-reference table before their limits can be checked.
    deferred: Vec<ObjectIdentifier>,
    decoded: u64,
}

/// Check every stream object in `data` before hayro reads any of it.
pub(super) fn before_parse(data: &[u8]) -> Result<Scan, String> {
    let ctx = ReaderContext::dummy();
    let mut scan = Scan {
        file_len: data.len(),
        encrypted: find(data, b"/Encrypt", 0).is_some(),
        streams: Vec::new(),
        deferred: Vec::new(),
        decoded: 0,
    };
    let mut endstreams = Vec::new();
    let mut endstream_at = 0;
    while let Some(found) = find(data, b"endstream", endstream_at) {
        endstreams.push(found);
        endstream_at = found + 1;
    }
    for found in object_headers(data) {
        if data[..found].ends_with(b"end") {
            continue;
        }
        let mut reader = Reader::new_with(data, found + 3);
        reader.skip_white_spaces_and_comments();
        if reader.peek_tag(b"<<").is_none() {
            continue;
        }
        let Some(dict) = reader.read_with_context::<Dict<'_>>(&ctx) else {
            continue;
        };
        // Resume after the dictionary rather than the stream data, so an
        // object hidden inside another stream's bytes is still found.
        reader.skip_white_spaces_and_comments();
        if reader.forward_tag(b"stream").is_none()
            || reader
                .forward_tag(b"\n")
                .or_else(|| reader.forward_tag(b"\r\n"))
                .or_else(|| reader.forward_tag(b"\r"))
                .is_none()
        {
            continue;
        }
        let start = reader.offset();
        if let Some(id) = object_id(&data[..found]) {
            scan.streams.push(id);
            if dict.get_ref(b"Filter").is_some()
                || dict.get_ref(b"F").is_some()
                || dict.get_ref(b"Length").is_some()
                || dict.get_ref(b"Width").is_some()
                || dict.get_ref(b"W").is_some()
                || dict.get_ref(b"Height").is_some()
                || dict.get_ref(b"H").is_some()
            {
                scan.deferred.push(id);
            }
        }
        let filters = check_dict(&dict, scan.file_len)?;
        // A direct dictionary is measured immediately; indirect values are
        // checked again after hayro resolves the cross-reference table.
        let end = stream_end(data, &dict, start, &endstreams);
        if !scan.encrypted {
            scan.measure(&filters, &data[start..], &data[start..end])?;
        }
    }
    Ok(scan)
}

impl Scan {
    /// Check the streams again through the parsed document, which resolves
    /// indirect sizes and decrypts data the first pass couldn't measure.
    pub(super) fn after_parse(mut self, pdf: &Pdf) -> Result<(), String> {
        let mut ids = if self.encrypted {
            std::mem::take(&mut self.streams)
        } else {
            Vec::new()
        };
        ids.extend(std::mem::take(&mut self.deferred));
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let Some(stream) = pdf.xref().get::<Stream<'_>>(id) else {
                continue;
            };
            let filters = check_dict(stream.dict(), self.file_len)?;
            let data = stream.raw_data();
            self.measure(&filters, &data, &data)?;
        }
        Ok(())
    }

    /// Count what the stream's one expanding filter decodes to. `tail` runs
    /// from the start of the stream data to the end of what is known; a
    /// compressed stream stops at its own end marker, so a misleading
    /// `/Length` can't hide data from the count. `body` is the stream as
    /// delimited, for the text filters that may come first.
    fn measure(&mut self, filters: &[Filter], tail: &[u8], body: &[u8]) -> Result<(), String> {
        let Some(index) = filters.iter().position(expands) else {
            return Ok(());
        };
        let input = if index == 0 {
            Cow::Borrowed(tail)
        } else {
            match decode_text_filters(&filters[..index], body) {
                Some(decoded) => Cow::Owned(decoded),
                // hayro can't decode it either, so nothing expands.
                None => return Ok(()),
            }
        };
        let decoded = match filters[index] {
            Filter::FlateDecode => inflated_len(&input, MAX_STREAM_DECODED),
            // Either code-width schedule, whichever `/EarlyChange` says.
            Filter::LzwDecode => lzw_len(&input, true, MAX_STREAM_DECODED).max(lzw_len(
                &input,
                false,
                MAX_STREAM_DECODED,
            )),
            _ => run_length_len(&input, MAX_STREAM_DECODED),
        };
        self.decoded += decoded;
        if decoded > MAX_STREAM_DECODED || self.decoded > MAX_DOCUMENT_DECODED {
            return Err(EXPANDS.into());
        }
        Ok(())
    }
}

/// The checks a stream dictionary alone allows; returns its filters, the
/// leading decryption filter left out.
fn check_dict(dict: &Dict<'_>, file_len: usize) -> Result<Vec<Filter>, String> {
    if dict
        .get::<i64>(b"Length")
        .is_some_and(|length| length < 0 || length as u64 > file_len as u64)
    {
        return Err(LENGTH.into());
    }
    let mut filters = filters(dict);
    if filters.first() == Some(&Filter::Crypt) {
        filters.remove(0);
    }
    // One expanding filter is bounded by measuring it; a second would
    // multiply whatever the first produced. Only text encodings may come
    // before it, since the measurement has to decode them first.
    if let Some(index) = filters.iter().position(expands)
        && (filters[index + 1..].iter().any(expands)
            || !filters[..index]
                .iter()
                .all(|filter| matches!(filter, Filter::AsciiHexDecode | Filter::Ascii85Decode)))
    {
        return Err(CHAINED.into());
    }
    if dict
        .get::<Name<'_>>(b"Subtype")
        .is_some_and(|subtype| &*subtype == b"Image")
    {
        let width = dict.get::<u32>(b"Width").or_else(|| dict.get(b"W"));
        let height = dict.get::<u32>(b"Height").or_else(|| dict.get(b"H"));
        if let (Some(width), Some(height)) = (width, height) {
            let pixels = u64::from(width) * u64::from(height);
            if pixels > MAX_IMAGE_PIXELS || pixels * image_components(dict) > MAX_IMAGE_BYTES {
                return Err(IMAGE.into());
            }
        }
    }
    Ok(filters)
}

/// A stream's filters as hayro reads them: one name, or an array of names
/// with unknown ones skipped.
fn filters(dict: &Dict<'_>) -> Vec<Filter> {
    if let Some(filter) = dict
        .get::<Name<'_>>(b"F")
        .or_else(|| dict.get::<Name<'_>>(b"Filter"))
        .and_then(|name| filter_named(&name))
    {
        return vec![filter];
    }
    dict.get::<Array<'_>>(b"F")
        .or_else(|| dict.get::<Array<'_>>(b"Filter"))
        .map(|array| {
            array
                .iter::<Name<'_>>()
                .filter_map(|name| filter_named(&name))
                .collect()
        })
        .unwrap_or_default()
}

fn filter_named(name: &[u8]) -> Option<Filter> {
    Some(match name {
        b"ASCIIHexDecode" | b"AHx" => Filter::AsciiHexDecode,
        b"ASCII85Decode" | b"A85" => Filter::Ascii85Decode,
        b"LZWDecode" | b"LZW" => Filter::LzwDecode,
        b"FlateDecode" | b"Fl" => Filter::FlateDecode,
        b"RunLengthDecode" | b"RL" => Filter::RunLengthDecode,
        b"CCITTFaxDecode" | b"CCF" => Filter::CcittFaxDecode,
        b"JBIG2Decode" => Filter::Jbig2Decode,
        b"DCTDecode" | b"DCT" => Filter::DctDecode,
        b"JPXDecode" => Filter::JpxDecode,
        b"Crypt" => Filter::Crypt,
        _ => return None,
    })
}

/// Filters whose output can be much larger than their input.
fn expands(filter: &Filter) -> bool {
    matches!(
        filter,
        Filter::FlateDecode | Filter::LzwDecode | Filter::RunLengthDecode
    )
}

/// Samples per pixel an image decodes to; four when the color space isn't
/// a simple named one.
fn image_components(dict: &Dict<'_>) -> u64 {
    if dict.get::<bool>(b"ImageMask") == Some(true) {
        return 1;
    }
    match dict
        .get::<Name<'_>>(b"ColorSpace")
        .or_else(|| dict.get::<Name<'_>>(b"CS"))
        .as_deref()
    {
        Some(b"DeviceGray" | b"G" | b"CalGray") => 1,
        Some(b"DeviceRGB" | b"RGB" | b"CalRGB" | b"Lab") => 3,
        _ => 4,
    }
}

/// Where a stream's data ends: at a direct `/Length` that lands on
/// `endstream`, as hayro reads it, otherwise at the next `endstream`.
fn stream_end(data: &[u8], dict: &Dict<'_>, start: usize, endstreams: &[usize]) -> usize {
    if let Some(end) = dict
        .get::<u32>(b"Length")
        .and_then(|length| start.checked_add(length as usize))
        .filter(|&end| {
            data.get(end..)
                .is_some_and(|rest| rest.trim_ascii_start().starts_with(b"endstream"))
        })
    {
        return end;
    }
    let next = endstreams.partition_point(|&at| at < start);
    endstreams.get(next).copied().unwrap_or(data.len())
}

/// Locate object keywords in PDF lexical text, skipping comments, literal
/// strings and hexadecimal strings. Searching for the byte sequence `obj`
/// directly treats examples embedded in page text as real objects and can
/// reject an otherwise valid document before hayro sees it.
fn object_headers(data: &[u8]) -> impl Iterator<Item = usize> + '_ {
    let mut at = 0;
    let mut literal_depth = 0usize;
    let mut escaped = false;
    let mut comment = false;
    let mut hex = false;
    std::iter::from_fn(move || {
        loop {
            while at < data.len() {
                let byte = data[at];
                if comment {
                    at += 1;
                    if matches!(byte, b'\n' | b'\r') {
                        comment = false;
                    }
                    continue;
                }
                if literal_depth > 0 {
                    at += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'(' {
                        literal_depth += 1;
                    } else if byte == b')' {
                        literal_depth -= 1;
                    }
                    continue;
                }
                if hex {
                    at += 1;
                    if byte == b'>' {
                        hex = false;
                    }
                    continue;
                }
                match byte {
                    b'%' => {
                        comment = true;
                        at += 1;
                    }
                    b'(' => {
                        literal_depth = 1;
                        at += 1;
                    }
                    b'<' if data.get(at + 1) == Some(&b'<') => {
                        // Dictionary opener `<<`, not a hexadecimal string.
                        at += 2;
                    }
                    b'<' => {
                        hex = true;
                        at += 1;
                    }
                    b'o' if data.get(at..at + 3) == Some(b"obj") => {
                        let end = at + 3;
                        let boundary = |byte: Option<&u8>| {
                            byte.is_none_or(|byte| byte.is_ascii_whitespace() || *byte == b'<')
                        };
                        let generation_end = at.checked_sub(1).map(|mut index| {
                            while data.get(index).is_some_and(u8::is_ascii_whitespace) {
                                index = index.saturating_sub(1);
                            }
                            index
                        });
                        let id_shape = generation_end
                            .and_then(|index| data.get(index))
                            .is_some_and(u8::is_ascii_digit)
                            && generation_end
                                .and_then(|index| index.checked_sub(1))
                                .and_then(|index| data.get(index))
                                .is_some_and(u8::is_ascii_whitespace);
                        let candidate = if id_shape && boundary(data.get(end)) {
                            object_id(&data[..at])
                        } else {
                            None
                        };
                        at = end;
                        if candidate.is_some() {
                            let header = at - 3;
                            // Do not lex arbitrary compressed/binary stream bytes
                            // as PDF text. The stream object itself was found by
                            // the lexical scan; its bytes are not object headers.
                            let object_end = find(data, b"endobj", at).unwrap_or(data.len());
                            if let Some(stream) =
                                find(data, b"stream", at).filter(|&p| p < object_end)
                                && let Some(endstream) = find(data, b"endstream", stream + 6)
                            {
                                at = endstream.saturating_add(b"endstream".len());
                                literal_depth = 0;
                                escaped = false;
                                comment = false;
                                hex = false;
                            }
                            return Some(header);
                        }
                    }
                    _ => at += 1,
                }
            }
            return None;
        }
    })
}

/// The `N G` before an `obj` keyword.
fn object_id(before: &[u8]) -> Option<ObjectIdentifier> {
    fn trailing_number(data: &[u8]) -> Option<(&[u8], &[u8])> {
        let data = data.trim_ascii_end();
        let digits = data
            .iter()
            .rev()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        let (rest, number) = data.split_at(data.len() - digits);
        (!number.is_empty()).then_some((rest, number))
    }
    fn parse<T: std::str::FromStr>(digits: &[u8]) -> Option<T> {
        std::str::from_utf8(digits).ok()?.parse().ok()
    }
    let (rest, generation) = trailing_number(before)?;
    let (_, number) = trailing_number(rest)?;
    Some(ObjectIdentifier::new(parse(number)?, parse(generation)?))
}

/// Decode the text encodings ahead of an expanding filter through hayro, as
/// a stream of their own. Their output is at most four times their input.
fn decode_text_filters(filters: &[Filter], body: &[u8]) -> Option<Vec<u8>> {
    let names = filters
        .iter()
        .map(|filter| match filter {
            Filter::AsciiHexDecode => "/ASCIIHexDecode",
            _ => "/ASCII85Decode",
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut bytes =
        format!("<< /Filter [{names}] /Length {} >>\nstream\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(b"\nendstream");
    let mut reader = Reader::new(&bytes);
    let stream = reader.read_with_context::<Stream<'_>>(&ReaderContext::dummy())?;
    stream.decoded().ok().map(Cow::into_owned)
}

/// What a zlib or raw deflate stream inflates to, counted up to just past
/// `cap` and never kept. hayro tries zlib first and raw deflate after.
fn inflated_len(data: &[u8], cap: u64) -> u64 {
    fn count(mut decoder: impl Read, cap: u64) -> (u64, bool) {
        let mut buffer = vec![0; 64 * 1024];
        let mut total = 0;
        loop {
            match decoder.read(&mut buffer) {
                Ok(0) => return (total, true),
                Ok(read) => {
                    total += read as u64;
                    if total > cap {
                        return (total, true);
                    }
                }
                Err(_) => return (total, false),
            }
        }
    }
    let (zlib, complete) = count(flate2::read::ZlibDecoder::new(data), cap);
    if complete {
        return zlib;
    }
    zlib.max(count(flate2::read::DeflateDecoder::new(data), cap).0)
}

/// What an LZW stream decodes to, following hayro's decoder but tracking
/// only the length of each table entry.
fn lzw_len(data: &[u8], early_change: bool, cap: u64) -> u64 {
    const CLEAR_TABLE: usize = 256;
    const EOD: usize = 257;
    const MAX_ENTRIES: usize = 4096;
    const INITIAL_SIZE: usize = 258;
    let code_length = |entries: usize| match entries + usize::from(early_change) {
        2048.. => 12,
        1024.. => 11,
        512.. => 10,
        _ => 9,
    };
    let mut lengths: Vec<u64> = vec![1; 256];
    lengths.extend([0, 0]);
    let mut reader = BitReader::new(data);
    let mut previous: Option<usize> = None;
    let mut total = 0;
    while total <= cap {
        let Some(code) = reader.read(code_length(lengths.len())) else {
            break;
        };
        match code as usize {
            CLEAR_TABLE => {
                lengths.truncate(INITIAL_SIZE);
                previous = None;
            }
            EOD => break,
            code if code < lengths.len() => {
                total += lengths[code];
                if let Some(previous) = previous
                    && lengths.len() < MAX_ENTRIES
                {
                    lengths.push(lengths[previous] + 1);
                }
                previous = Some(code);
            }
            code if code == lengths.len() => {
                let Some(prior) = previous else {
                    break;
                };
                if lengths.len() >= MAX_ENTRIES {
                    break;
                }
                lengths.push(lengths[prior] + 1);
                total += lengths[code];
                previous = Some(code);
            }
            _ => break,
        }
    }
    total
}

/// What a run-length stream decodes to.
fn run_length_len(data: &[u8], cap: u64) -> u64 {
    let mut total = 0;
    let mut at = 0;
    while let Some(&length) = data.get(at)
        && total <= cap
    {
        at += 1;
        match length {
            128 => break,
            0..=127 => {
                let run = usize::from(length) + 1;
                if at + run > data.len() {
                    break;
                }
                at += run;
                total += run as u64;
            }
            _ => {
                if at >= data.len() {
                    break;
                }
                at += 1;
                total += 257 - u64::from(length);
            }
        }
    }
    total
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A PDF whose one page draws `content`, with `extra` objects appended
    /// (numbered from 5) and a correct cross-reference table.
    fn pdf_with(content: &[u8], extra: &[Vec<u8>]) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_vec(),
            stream(b"", content),
        ];
        objects.extend(extra.iter().cloned());
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// A stream object with `entries` added to its dictionary.
    fn stream(entries: &[u8], data: &[u8]) -> Vec<u8> {
        let mut object = format!("<< /Length {} ", data.len()).into_bytes();
        object.extend_from_slice(entries);
        object.extend_from_slice(b" >>\nstream\n");
        object.extend_from_slice(data);
        object.extend_from_slice(b"\nendstream");
        object
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn check(pdf: &[u8]) -> Result<(), String> {
        let scan = before_parse(pdf)?;
        let parsed = Pdf::new(pdf.to_vec()).map_err(|_| "unparsable".to_string())?;
        scan.after_parse(&parsed)
    }

    #[test]
    fn ordinary_documents_pass() {
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(
                b"/Type /XObject /Subtype /Image /Width 640 /Height 480 \
                  /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
                &zlib(&vec![0; 640 * 480 * 3]),
            )],
        );
        assert_eq!(check(&pdf), Ok(()));
        // A text encoding may come ahead of the compression.
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(
                b"/Filter [/ASCIIHexDecode /FlateDecode]",
                hex(&zlib(b"hello")).as_bytes(),
            )],
        );
        assert_eq!(check(&pdf), Ok(()));
    }

    #[test]
    fn indirect_filters_are_checked_after_cross_references_resolve() {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        let buffer = [0u8; 64 * 1024];
        for _ in 0..(MAX_STREAM_DECODED / buffer.len() as u64 + 1) {
            encoder.write_all(&buffer).unwrap();
        }
        let compressed = encoder.finish().unwrap();
        let mut pdf = pdf_with(
            b"",
            &[
                stream(b"/Filter 6 0 R", &compressed),
                b"/FlateDecode".to_vec(),
            ],
        );
        // Point the page at object 5, whose filter is the indirect name in
        // object 6. The direct pass cannot resolve that name.
        let at = find(&pdf, b"/Contents 4 0 R", 0).unwrap();
        pdf[at + b"/Contents ".len()] = b'5';
        assert_eq!(check(&pdf), Err(EXPANDS.into()));
    }

    #[test]
    fn object_examples_inside_literal_text_are_not_scanned_as_streams() {
        let pdf = pdf_with(
            b"BT (Example: 7 0 obj << /Length 999999999 >> stream\\ntext endstream endobj) Tj ET",
            &[],
        );
        assert_eq!(check(&pdf), Ok(()));
    }

    fn hex(data: &[u8]) -> String {
        let mut text: String = data.iter().map(|byte| format!("{byte:02X}")).collect();
        text.push('>');
        text
    }

    #[test]
    fn chained_compression_is_refused() {
        let inner = zlib(&vec![0; 1024]);
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(
                b"/Filter [/FlateDecode /FlateDecode]",
                &zlib(&inner),
            )],
        );
        assert_eq!(check(&pdf), Err(CHAINED.into()));
        // The abbreviated names count too.
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(b"/Filter [/Fl /LZW]", &inner)],
        );
        assert_eq!(check(&pdf), Err(CHAINED.into()));
    }

    #[test]
    fn oversized_images_are_refused() {
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(
                b"/Type /XObject /Subtype /Image /Width 100000 /Height 100000 \
                  /ColorSpace /DeviceGray /BitsPerComponent 8",
                b"\0",
            )],
        );
        assert_eq!(check(&pdf), Err(IMAGE.into()));
        // An image whose size is an indirect reference is caught once the
        // document resolves it.
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[
                stream(
                    b"/Type /XObject /Subtype /Image /Width 6 0 R /Height 6 0 R \
                      /ColorSpace /DeviceRGB /BitsPerComponent 8",
                    b"\0",
                ),
                b"20000".to_vec(),
            ],
        );
        assert!(before_parse(&pdf).is_ok());
        assert_eq!(check(&pdf), Err(IMAGE.into()));
    }

    #[test]
    fn decompression_bombs_are_measured_before_parsing() {
        // 260 MiB of zeros compresses to well under a MiB.
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        let zeros = vec![0; 1024 * 1024];
        for _ in 0..260 {
            encoder.write_all(&zeros).unwrap();
        }
        let bomb = encoder.finish().unwrap();
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[stream(b"/Filter /FlateDecode", &bomb)],
        );
        assert_eq!(before_parse(&pdf).err(), Some(EXPANDS.into()));
    }

    #[test]
    fn absurd_lengths_are_refused() {
        let pdf = pdf_with(
            b"0 0 0 rg 20 20 60 60 re f",
            &[b"<< /Length 99999999 >>\nstream\nabc\nendstream".to_vec()],
        );
        assert_eq!(before_parse(&pdf).err(), Some(LENGTH.into()));
    }

    #[test]
    fn counting_decoders_match_the_data() {
        assert_eq!(inflated_len(&zlib(&[7; 5000]), u64::MAX), 5000);
        // hayro's LZW example: ten bytes.
        let lzw = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(lzw_len(&lzw, true, u64::MAX), 10);
        // A literal run of three, a repeat of 127, then the end marker.
        assert_eq!(
            run_length_len(&[2, b'a', b'b', b'c', 130, b'z', 128, 0, 0], u64::MAX),
            3 + 127
        );
        assert_eq!(object_id(b"\n12 0 "), Some(ObjectIdentifier::new(12, 0)));
        assert_eq!(object_id(b"/Name "), None);
    }
}
