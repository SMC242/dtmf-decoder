use ogg::reading as ogg;
use std::fs;
use std::path::Path;

// From https://opus-codec.org/examples/
const FILE_PATH: &str = "/home/eilidhm/Downloads/ehren-paper_lights-96.opus";

/// Sampling rate in Hertz
/// NOTE: the Opus codec only supports these sampling rates
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum SamplingRate {
    Narrowband = 8_000,
    Mediumband = 12_000,
    Wideband = 16_000,
    SuperWideband = 24_000,
    Fullband = 48_000,
}

#[derive(Debug)]
enum DecodeDtmfError {
    IoError(std::io::Error),
    SignalReadError(DecodeTimesliceError),
}

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
) -> Result<Vec<i16>, DecodeTimesliceError> {
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
                dbg!(packet.data.len(), packet_data.len());

                let samples = decoder
                    .decode(&packet.data, &mut packet_data, true)
                    .map_err(DecodeTimesliceError::DecodeError)?;
                total_samples += u64::try_from(samples)
                    .expect("Converting sample count to u64 should be ok on 64-bit systems");
                signal.extend(packet_data);
            }
            Ok(None) => return Ok(signal),
            Err(err) => return Err(DecodeTimesliceError::OggFormatError(err)),
        };
    }
    Ok(signal)
}

fn main() -> Result<(), DecodeDtmfError> {
    // Obtained by inspecting the file with get-sampling-rate.sh
    let sampling_rate = SamplingRate::Fullband;

    let u32_sampling_rate = u32::from(sampling_rate as u16);
    // Stereo also from inspection
    let mut decoder = opus::Decoder::new(u32_sampling_rate, opus::Channels::Stereo)
        .expect("Initialising the decoder should succeed");
    let mut stream = read_ogg(Path::new(FILE_PATH)).map_err(DecodeDtmfError::IoError)?;
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
