use ogg::reading as ogg;
use std::fs;
use std::path::Path;

/// Sampling rate in bits per second
type SamplingRate = u64;

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
pub fn decode_timeslice(
    sampling_rate: SamplingRate,
    timeslice: std::time::Duration,
    decoder: &mut opus::Decoder,
    stream: &mut ogg::PacketReader<fs::File>,
) -> Result<Vec<i16>, DecodeTimesliceError> {
    let mut signal = Vec::new();
    // Assumes no packet loss
    let expected_samples = sampling_rate * timeslice.as_secs();
    let mut total_samples = 0;
    while total_samples < expected_samples {
        match stream.read_packet() {
            Ok(Some(packet)) => {
                let samples = decoder
                    .decode(&packet.data, &mut signal, false)
                    .map_err(DecodeTimesliceError::DecodeError)?;
                total_samples += u64::try_from(samples)
                    .expect("Converting sample count to u64 should be ok on 64-bit systems");
            }
            Ok(None) => return Ok(signal),
            Err(err) => return Err(DecodeTimesliceError::OggFormatError(err)),
        };
    }
    Ok(signal)
}

fn main() -> Result<(), std::io::Error> {
    const FILE_PATH: &str = "~/Downloads/ehren-paper_lights-96.opus";
    const SAMPLING_RATE: SamplingRate = 96 * 1024;

    let u32_sampling_rate =
        u32::try_from(SAMPLING_RATE).expect("A reasonable sampling rate will fit in a u32");
    let mut decoder = opus::Decoder::new(u32_sampling_rate, opus::Channels::Mono)
        .expect("Initialising the decoder should succeed");
    let mut stream = read_ogg(Path::new(FILE_PATH))?;
    let first_10s = decode_timeslice(
        SAMPLING_RATE,
        std::time::Duration::from_secs(10),
        &mut decoder,
        &mut stream,
    )
    .map_err(|err| std::io::Error::new(ErrorKind::Other, err))?;
    dbg!("{0:?}", first_10s);
    Ok(())
}
