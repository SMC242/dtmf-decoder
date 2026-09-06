use ::ogg::PacketReader;
use ogg::reading as ogg;
use std::fs;
use std::io::{Error as IoError, Read, Seek};
use std::num::NonZeroU8;
use std::path::Path;

// From https://opus-codec.org/examples/
const FILE_PATH: &str = "/home/eilidhm/Downloads/ehren-paper_lights-96.opus";

type OpusSamples = [i16];

/// Sampling rate in Hertz
/// NOTE: the Opus codec only supports these sampling rates
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum SamplingRate {
    Narrowband = 8_000,
    Mediumband = 12_000,
    Wideband = 16_000,
    SuperWideband = 24_000,
    // NOTE: not officially supported. The decoder will upsample this
    // See https://github.com/xiph/opus/issues/43
    Cd = 44_100,
    Fullband = 48_000,
}

impl SamplingRate {
    pub fn to_hertz(&self) -> u32 {
        *self as u32
    }
}

impl TryFrom<u32> for SamplingRate {
    type Error = &'static str;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        const ALL: &[SamplingRate] = &[
            SamplingRate::Narrowband,
            SamplingRate::Mediumband,
            SamplingRate::Wideband,
            SamplingRate::SuperWideband,
            SamplingRate::Cd,
            SamplingRate::Fullband,
        ];
        ALL.iter().find(|&variant| variant.to_hertz() == value).copied()
            .ok_or("Invalid sampling rate for the Opus codec. See https://www.rfc-editor.org/info/rfc7845/#section-5.1")
    }
}

#[derive(Debug)]
enum DecodeDtmfError {
    IoError(IoError),
    OpusParseError(OpusHeaderParseError),
    OpusDecodeError(opus::Error),
    OggFormatError(ogg::OggReadError),
    EndOfStream,
}

type SignalSlice = Vec<i16>;

pub fn read_ogg(path: &Path) -> Result<ogg::PacketReader<fs::File>, IoError> {
    let file = fs::File::open(path)?;
    Ok(ogg::PacketReader::new(file))
}

/// Decode the Opus packets contained within the ogg stream for the next `timeslice` (E.G next 20ms).
/// This will return less than the expected number of samples (`sampling_rate * timeslice`) if the stream ends
/// or more if more data is in the packets than desired
pub fn decode_timeslice(
    sampling_rate: SamplingRate,
    timeslice: std::time::Duration,
    decoder: &mut opus::Decoder,
    stream: &mut ogg::PacketReader<fs::File>,
) -> Result<SignalSlice, DecodeDtmfError> {
    const DECODE_FEC: bool = false;

    let mut signal = Vec::new();
    // Assumes no packet loss
    let expected_samples = u64::from(sampling_rate as u16) * timeslice.as_secs();
    let mut total_samples = 0;
    while total_samples < expected_samples {
        match stream.read_packet() {
            Ok(Some(packet)) => {
                let mut packet_data: Vec<i16> = vec![0; packet.data.len()];
                dbg!(packet.data.len());
                dbg!(packet_data.len());

                let frame_size = decoder
                    .decode(&packet.data, &mut packet_data, DECODE_FEC)
                    .map_err(DecodeDtmfError::OpusDecodeError)?;
                total_samples += u64::try_from(frame_size)
                    .expect("Converting sample count to u64 should be ok on 64-bit systems");
                signal.extend(packet_data);
            }
            Ok(None) => return Ok(signal),
            Err(err) => return Err(DecodeDtmfError::OggFormatError(err)),
        };
    }
    Ok(signal)
}

// See https://www.rfc-editor.org/info/rfc7845/#section-5

#[derive(Debug)]
pub struct OpusStreamMetadata {
    pub version: u8,
    pub channel_count: NonZeroU8,
    pub preskip: usize,
    pub input_sample_rate_hz: SamplingRate,
    pub output_gain_db: i16,
}

#[derive(Debug)]
pub enum OpusHeaderParseError {
    MissingOpusHead,
    MalformedOpusHead(String),
    UnsupportedVersion(u8),
    MissingOpusTags,
    ReadFailed(ogg::OggReadError),
    /// Only mono and stereo audio is supported as this application focuses on phone calls.
    /// 5.1 surround sound is unlikely in this case and channel family 255 shouldn't be used by
    /// "general-purpose players". See https://www.rfc-editor.org/info/rfc7845/#section-5.1.1.1
    UnsupportedChannelFamily,
    NotOpus,
}

fn read_opus_field<const N: usize, T, F: FnOnce([u8; N]) -> T, R: std::io::Read>(
    converter: F,
    err_msg: &'static str,
    cur: &mut R,
) -> Result<T, OpusHeaderParseError> {
    let mut buf = [0u8; N];
    cur.read_exact(&mut buf)
        .or(Err(OpusHeaderParseError::MalformedOpusHead(
            err_msg.to_string(),
        )))?;
    Ok(converter(buf))
}

fn read_opus_field_single<T, F: FnOnce(u8) -> T, R: std::io::Read>(
    converter: F,
    err_msg: &'static str,
    cur: &mut R,
) -> Result<T, OpusHeaderParseError> {
    read_opus_field(|xs: [u8; 1]| converter(xs[0]), err_msg, cur)
}

fn offset_opus_cursor<R: std::io::Seek>(
    offset: i64,
    err_msg: &'static str,
    cur: &mut R,
) -> Result<(), OpusHeaderParseError> {
    cur.seek_relative(offset)
        .or(Err(OpusHeaderParseError::MalformedOpusHead(
            err_msg.to_string(),
        )))?;
    Ok(())
}

fn parse_opus_headers<T: std::io::Read + std::io::Seek>(
    reader: &mut ogg::PacketReader<T>,
) -> Result<OpusStreamMetadata, OpusHeaderParseError> {
    let opus_head_packet = reader
        .read_packet()
        .map_err(OpusHeaderParseError::ReadFailed)
        .map(|p| p.ok_or(OpusHeaderParseError::MissingOpusHead))??;
    println!("OpusHead packet raw: {0:x?}", &opus_head_packet.data);

    // Not an Opus packet
    if !opus_head_packet.data.starts_with(b"OpusHead") {
        return Err(OpusHeaderParseError::NotOpus);
    }

    let mut cur = std::io::Cursor::new(opus_head_packet.data);

    // Cursor magic from https://github.com/karx1/opusmeta/blob/master/src/lib.rs#L242
    offset_opus_cursor(8, "Unexpected EOF at OpusHead version", &mut cur)?;
    let major_version_raw =
        read_opus_field_single(u8::from_le, "Unexpected EOF at OpusHead version", &mut cur)?;
    // Ignore the minor version as required by the RFC
    let major_version = major_version_raw & 0b00001111;
    if major_version != 1 {
        return Err(OpusHeaderParseError::UnsupportedVersion(major_version));
    }

    let channel_count = read_opus_field_single(
        |x| {
            NonZeroU8::try_from(x).or(Err(OpusHeaderParseError::MalformedOpusHead(
                "Channel count can't be 0".to_string(),
            )))
        },
        "Missing channel count",
        &mut cur,
    )??;

    // All mutli-byte values will be little endian due to RFC
    let preskip = read_opus_field(
        u16::from_le_bytes,
        "Unexpected end of stream in preskip header",
        &mut cur,
    )?;

    let input_sample_rate_hz = read_opus_field(
        u32::from_le_bytes,
        "Unexpected end of stream in input sample rate header",
        &mut cur,
    )?;
    let sampling_rate = SamplingRate::try_from(input_sample_rate_hz).or(Err(
        OpusHeaderParseError::MalformedOpusHead(format!(
            "Invalid sampling rate '{input_sample_rate_hz}' for the Opus codec"
        )),
    ))?;

    let output_gain_db = read_opus_field(
        i16::from_le_bytes,
        "Unexpected end of stream in output gain header",
        &mut cur,
    )?;

    let channel_mapping_family_raw =
        read_opus_field_single(u8::from_le, "Missing channel mapping family", &mut cur)?;
    let _channel_mapping_family = match channel_mapping_family_raw {
        0 => 0,
        1.. => return Err(OpusHeaderParseError::UnsupportedChannelFamily),
    };

    // I can't be bothered parsing the channel mapping table so I will demux it
    // which should be okay according to https://www.rfc-editor.org/info/rfc7845/#section-5.1.1
    // Phone audio is unlikely to be stereo anyway

    // Skip the tags as that metadata isn't useful
    let _tags_packet = reader
        .read_packet()
        .map_err(OpusHeaderParseError::ReadFailed)
        .map(|p| p.ok_or(OpusHeaderParseError::MissingOpusTags))??;

    Ok(OpusStreamMetadata {
        version: major_version,
        channel_count,
        preskip: preskip.into(),
        input_sample_rate_hz: sampling_rate,
        output_gain_db,
    })
}

// Skip the number of samples in `headers.preskip`
// May return some decoded samples if the preskip ends mid-packet
// See https://www.rfc-editor.org/info/rfc7845/#section-4.2
fn do_preskip<T: Read + Seek>(
    headers: &OpusStreamMetadata,
    buffer: &mut OpusSamples,
    decoder: &mut opus::Decoder,
    stream: &mut PacketReader<T>,
) -> Result<Option<Vec<i16>>, DecodeDtmfError> {
    println!("Doing preskip for {0} samples", headers.preskip);

    let mut samples_seen: usize = 0;
    let mut res = read_opus_packet(buffer, decoder, stream);
    while let Ok(slice) = res
        && samples_seen <= headers.preskip
    {
        samples_seen += slice.len();
        println!("Skipping samples {slice:x?}");
        res = read_opus_packet(buffer, decoder, stream);
    }

    res.map(|slice| {
        let extra_samples = samples_seen - headers.preskip;
        if extra_samples > 0 {
            Some(slice.iter().copied().rev().take(extra_samples).collect())
        } else {
            None
        }
    })
}

fn calc_max_frame_size(sampling_rate: u32, channels: opus::Channels) -> usize {
    // The longest packets are 120ms
    const LONGEST_PACKET_MS: u32 = 120;
    let samples_per_channel = (LONGEST_PACKET_MS * sampling_rate) / 1000;
    usize::try_from(samples_per_channel * channels as u32)
        .expect("The samples per channel should fit in usize")
}

/// Read a packet into `buffer` and return a slice of it containing the decoded samples.
/// This returns a slice because the length of Opus packets is variable
fn read_opus_packet<'a, T: Read + Seek>(
    buffer: &'a mut OpusSamples,
    decoder: &mut opus::Decoder,
    stream: &mut PacketReader<T>,
) -> Result<&'a OpusSamples, DecodeDtmfError> {
    let packet = stream
        .read_packet()
        .map_err(|err| {
            DecodeDtmfError::IoError(IoError::other(format!(
                "Failed to read OGG packet due to {err:?}"
            )))
        })?
        .ok_or(DecodeDtmfError::EndOfStream)?;

    match decoder.decode(&packet.data, buffer, false) {
        Ok(sample_count) => Ok(&buffer[0..sample_count]),
        Err(e) => Err(DecodeDtmfError::OpusDecodeError(e)),
    }
}

fn print_packets<T: Read + Seek>(decoder: &mut opus::Decoder, stream: &mut PacketReader<T>) {
    let max_frame_size = calc_max_frame_size(
        decoder
            .get_sample_rate()
            .expect("Can get sampling rate from Opus decoder"),
        opus::Channels::Mono,
    );

    while let Some(packet) = stream.read_packet().transpose() {
        match packet {
            Ok(p) => {
                let mut output = vec![0; max_frame_size];
                match decoder.decode(&p.data, &mut output, false) {
                    Ok(samples_decoded) => {
                        println!("Packet data: {0:x?}", &output[0..samples_decoded])
                    }
                    Err(err) => {
                        dbg!(err);
                    }
                }
            }
            Err(err) => {
                dbg!(err);
            }
        }
    }
}

fn main() -> Result<(), DecodeDtmfError> {
    // The Opus RFC says to decode at 48 KHz if the hardware supports it
    // regardless of the source sampling rate
    // See https://www.rfc-editor.org/info/rfc6716/#section-2
    const SAMPLING_RATE: SamplingRate = SamplingRate::Fullband;
    // Mono always because phone mic audio is expected. If it happens to be unexpected
    // audio (E.G a song), the decoder will just demux the stream
    const CHANNELS: opus::Channels = opus::Channels::Mono;

    let u32_sampling_rate = u32::from(SAMPLING_RATE as u16);
    let mut decoder = opus::Decoder::new(u32_sampling_rate, CHANNELS)
        .expect("Initialising the decoder should succeed");
    let mut stream = read_ogg(Path::new(FILE_PATH)).map_err(DecodeDtmfError::IoError)?;

    println!("Parsing headers");
    let stream_meta = parse_opus_headers(&mut stream).map_err(DecodeDtmfError::OpusParseError)?;
    dbg!(&stream_meta);

    // print_packets(&mut decoder, &mut stream);
    let frame_size = calc_max_frame_size(u32_sampling_rate, CHANNELS);
    let mut buffer = vec![0; frame_size];
    let mut signal: Vec<i16> = Vec::with_capacity(frame_size);
    if let Some(extra_samples) = do_preskip(&stream_meta, &mut buffer, &mut decoder, &mut stream)? {
        println!(
            "Handled {0} extra samples after preskip.\n{extra_samples:x?}",
            extra_samples.len()
        );
        signal.extend_from_slice(extra_samples.as_ref())
    }

    let first_10s = decode_timeslice(
        SAMPLING_RATE,
        std::time::Duration::from_secs(10),
        &mut decoder,
        &mut stream,
    )?;
    dbg!("{0:?}", first_10s);
    Ok(())
}
