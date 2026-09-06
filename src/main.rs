use ogg::reading as ogg;
use std::fs;
use std::io::Error as IoError;
use std::path::Path;
mod opus_reader;

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
pub enum DecodeDtmfError {
    IoError(IoError),
    OpusParseError(opus_reader::OpusHeaderParseError),
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

                let frame_size = decoder
                    .decode(&packet.data, &mut packet_data, DECODE_FEC)
                    .map_err(DecodeDtmfError::OpusDecodeError)?;
                total_samples += u64::try_from(frame_size)
                    .expect("Converting sample count to u64 should be ok on 64-bit systems");
                signal.extend(packet_data);
            }
            Ok(Option::None) => return Ok(signal),
            Err(err) => return Err(DecodeDtmfError::OggFormatError(err)),
        };
    }
    Ok(signal)
}

fn main() -> Result<(), DecodeDtmfError> {
    // The Opus RFC says to decode at 48 KHz if the hardware supports it
    // regardless of the source sampling rate
    // See https://www.rfc-editor.org/info/rfc6716/#section-2
    const SAMPLING_RATE: SamplingRate = SamplingRate::Fullband;
    // Mono always because phone mic audio is expected. If it happens to be unexpected
    // audio (E.G a song), the decoder will just demux the stream
    const CHANNELS: opus::Channels = opus::Channels::Mono;

    let u32_sampling_rate = SAMPLING_RATE as u32;
    let decoder = opus::Decoder::new(u32_sampling_rate, CHANNELS)
        .expect("Initialising the decoder should succeed");
    let mut stream = read_ogg(Path::new(FILE_PATH)).map_err(DecodeDtmfError::IoError)?;

    println!("Parsing headers");
    let stream_meta =
        opus_reader::parse_opus_headers(&mut stream).map_err(DecodeDtmfError::OpusParseError)?;
    dbg!(&stream_meta);

    // print_packets(&mut decoder, &mut stream);
    let mut reader = opus_reader::OpusReader::new(SAMPLING_RATE, CHANNELS, decoder, stream);
    let mut signal: Vec<i16> = Vec::with_capacity(reader.get_buffer_size());
    if let Some(extra_samples) = reader.do_preskip(&stream_meta)? {
        println!(
            "Handled {0} extra samples after preskip.\n{extra_samples:x?}",
            extra_samples.len()
        );
        signal.extend_from_slice(extra_samples.as_ref())
    }

    Ok(())
}
