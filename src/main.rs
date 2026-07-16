use ogg::reading as ogg;
use std::fs;
use std::num::NonZeroU8;
use std::path::Path;

// From https://opus-codec.org/examples/
const FILE_PATH: &str = "/home/eilidhm/Downloads/ehren-paper_lights-96.opus";

/// Sampling rate in Hertz
/// NOTE: the Opus codec only supports these sampling rates
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum SamplingRate {
    Narrowband = 8_000,
    Mediumband = 12_000,
    Wideband = 16_000,
    SuperWideband = 24_000,
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
            SamplingRate::Fullband,
        ];
        ALL.iter().find(|&variant| variant.to_hertz() == value).copied()
            .ok_or("Invalid sampling rate for the Opus codec. See https://www.rfc-editor.org/info/rfc7845/#section-5.1")
    }
}

#[derive(Debug)]
enum DecodeDtmfError {
    IoError(std::io::Error),
    SignalReadError(DecodeTimesliceError),
    OpusParseError(OpusHeaderParseError),
}

type SignalSlice = Vec<i16>;

#[derive(Debug)]
pub enum DecodeTimesliceError {
    DecodeError(opus::Error),
    OggFormatError(ogg::OggReadError),
}

pub fn read_ogg(path: &Path) -> Result<ogg::PacketReader<fs::File>, std::io::Error> {
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
) -> Result<SignalSlice, DecodeTimesliceError> {
    const DECODE_FEC: bool = false;

    let mut signal = Vec::new();
    // Assumes no packet loss
    let expected_samples = u64::from(sampling_rate as u16) * timeslice.as_secs();
    let mut total_samples = 0;
    while total_samples < expected_samples {
        match stream.read_packet() {
            Ok(Some(packet)) => {
                dbg!(&packet.data);
                let mut packet_data: Vec<i16> = vec![0; packet.data.len()];
                dbg!(&packet_data);
                println!("{0:x?}", packet.data);
                dbg!(packet.data.len(), packet_data.len());

                let frame_size = decoder
                    .decode(&packet.data, &mut packet_data, DECODE_FEC)
                    .map_err(DecodeTimesliceError::DecodeError)?;
                total_samples += u64::try_from(frame_size)
                    .expect("Converting sample count to u64 should be ok on 64-bit systems");
                signal.extend(packet_data);
            }
            Ok(None) => return Ok(signal),
            Err(err) => return Err(DecodeTimesliceError::OggFormatError(err)),
        };
    }
    Ok(signal)
}

// See https://www.rfc-editor.org/info/rfc7845/#section-5

#[derive(Debug)]
pub struct OpusStreamMetadata {
    pub version: u8,
    pub channel_count: NonZeroU8,
    pub preskip: u16,
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
}

fn read_multi_byte<const N: usize, T, E, F: FnOnce([u8; N]) -> T>(
    converter: F,
    err: E,
    bytes: &[u8],
) -> Result<T, E> {
    <[u8; N]>::try_from(bytes).map_err(|_| err).map(converter)
}

fn parse_opus_headers<T: std::io::Read + std::io::Seek>(
    reader: &mut ogg::PacketReader<T>,
) -> Result<OpusStreamMetadata, OpusHeaderParseError> {
    let opus_head_packet = reader
        .read_packet()
        .map_err(OpusHeaderParseError::ReadFailed)
        .map(|p| p.ok_or(OpusHeaderParseError::MissingOpusHead))??;

    // Ignore the minor version as required by the RFC
    let version = opus_head_packet.data[1] & 0b11110000;
    if version != 1 {
        return Err(OpusHeaderParseError::UnsupportedVersion(version));
    }

    let channel_count = NonZeroU8::try_from(opus_head_packet.data[2]).map_err(|_| {
        OpusHeaderParseError::MalformedOpusHead("Invalid channel count: 0".to_string())
    })?;

    // All mutli-byte values will be little endian due to RFC
    let preskip = read_multi_byte(
        u16::from_le_bytes,
        OpusHeaderParseError::MalformedOpusHead(
            "Unexpected end of stream in preskip header".to_string(),
        ),
        &opus_head_packet.data[3..4],
    )?;

    let input_sample_rate_hz = read_multi_byte(
        u32::from_le_bytes,
        OpusHeaderParseError::MalformedOpusHead(
            "Unexpected end of stream in input sample rate header".to_string(),
        ),
        &opus_head_packet.data[5..9],
    )?;
    let sampling_rate = SamplingRate::try_from(input_sample_rate_hz).or(Err(
        OpusHeaderParseError::MalformedOpusHead(format!(
            "Invalid sampling rate '{input_sample_rate_hz}' for the Opus codec"
        )),
    ))?;

    let output_gain_db = read_multi_byte(
        i16::from_le_bytes,
        OpusHeaderParseError::MalformedOpusHead(
            "Unexpected end of stream in output gain header".to_string(),
        ),
        &opus_head_packet.data[9..13],
    )?;

    let _channel_mapping_family = match opus_head_packet.data[14] {
        0 => 0,
        1.. => return Err(OpusHeaderParseError::UnsupportedChannelFamily),
    };

    // I can't be bothered parsing the channel mapping table so I will demux it
    // which should be okay according to https://www.rfc-editor.org/info/rfc7845/#section-5.1.1
    // Phone audio is unlikely to be stereo anyway

    // TODO: skip the OpusTags. Not important for this application
    let _tags_packet = reader
        .read_packet()
        .map_err(OpusHeaderParseError::ReadFailed)
        .map(|p| p.ok_or(OpusHeaderParseError::MissingOpusTags))??;
    Ok(OpusStreamMetadata {
        version,
        channel_count,
        preskip,
        input_sample_rate_hz: sampling_rate,
        output_gain_db,
    })
}

fn main() -> Result<(), DecodeDtmfError> {
    // Obtained by inspecting the file with get-sampling-rate.sh
    let sampling_rate = SamplingRate::Fullband;

    let u32_sampling_rate = u32::from(sampling_rate as u16);
    // Stereo also from inspection
    let mut decoder = opus::Decoder::new(u32_sampling_rate, opus::Channels::Stereo)
        .expect("Initialising the decoder should succeed");
    let mut stream = read_ogg(Path::new(FILE_PATH)).map_err(DecodeDtmfError::IoError)?;

    let stream_meta = parse_opus_headers(&mut stream).map_err(DecodeDtmfError::OpusParseError)?;
    dbg!(stream_meta);

    let first_10s = decode_timeslice(
        sampling_rate,
        std::time::Duration::from_secs(10),
        &mut decoder,
        &mut stream,
    )
    .map_err(DecodeDtmfError::SignalReadError)?;
    dbg!("{0:?}", first_10s);
    Ok(())
}
