use crate::{DecodeDtmfError, OpusSamples, SamplingRate};
use ogg::PacketReader;
use std::io::{Error as IoError, Read, Seek};
use std::num::NonZeroU8;

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

pub fn parse_opus_headers<T: std::io::Read + std::io::Seek>(
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

/// Handles reading and decoding Opus packets from the OGG stream
pub struct OpusReader<T: Read + Seek> {
    buffer: Vec<i16>,
    decoder: opus::Decoder,
    stream: PacketReader<T>,
}

impl<T: Read + Seek> OpusReader<T> {
    pub fn new(
        sampling_rate: SamplingRate,
        channels: opus::Channels,
        decoder: opus::Decoder,
        stream: PacketReader<T>,
    ) -> Self {
        let buffer = vec![0; Self::calc_max_frame_size(sampling_rate, channels)];
        Self {
            buffer,
            decoder,
            stream,
        }
    }

    /// Read a packet into `buffer` and return a slice of it containing the decoded samples.
    /// This returns a slice because the length of Opus packets is variable
    pub fn read_opus_packet<'a>(&'a mut self) -> Result<&'a OpusSamples, DecodeDtmfError> {
        let packet = self
            .stream
            .read_packet()
            .map_err(|err| {
                DecodeDtmfError::IoError(IoError::other(format!(
                    "Failed to read OGG packet due to {err:?}"
                )))
            })?
            .ok_or(DecodeDtmfError::EndOfStream)?;

        match self.decoder.decode(&packet.data, &mut self.buffer, false) {
            Ok(sample_count) => Ok(&self.buffer[0..sample_count]),
            Err(e) => Err(DecodeDtmfError::OpusDecodeError(e)),
        }
    }

    // Skip the number of samples in `headers.preskip`
    // May return some decoded samples if the preskip ends mid-packet
    // See https://www.rfc-editor.org/info/rfc7845/#section-4.2
    pub fn do_preskip(
        &mut self,
        headers: &OpusStreamMetadata,
    ) -> Result<Option<Vec<i16>>, DecodeDtmfError> {
        println!("Doing preskip for {0} samples", headers.preskip);
        let mut samples_seen: usize = 0;

        loop {
            match self.read_opus_packet() {
                Ok(slice) if samples_seen < headers.preskip => {
                    samples_seen += slice.len();
                    println!("Skipping samples {slice:x?}");
                    continue;
                }
                Ok(slice) if samples_seen >= headers.preskip => {
                    let extra_samples = samples_seen - headers.preskip;
                    if extra_samples > 0 {
                        return Ok(Some(
                            slice.iter().copied().rev().take(extra_samples).collect(),
                        ));
                    } else {
                        return Ok(None);
                    }
                }
                Err(err) => return Err(err),
                Ok(_) => panic!("Non-overlapping if condition on samples_seen"),
            }
        }
    }

    fn calc_max_frame_size(sampling_rate: SamplingRate, channels: opus::Channels) -> usize {
        // The longest packets are 120ms
        const LONGEST_PACKET_MS: u32 = 120;
        let samples_per_channel = (LONGEST_PACKET_MS * sampling_rate as u32) / 1000;
        usize::try_from(samples_per_channel * channels as u32)
            .expect("The samples per channel should fit in usize")
    }

    pub fn print_packets(&mut self) {
        while let Some(packet) = self.stream.read_packet().transpose() {
            match packet {
                Ok(p) => match self.decoder.decode(&p.data, &mut self.buffer, false) {
                    Ok(samples_decoded) => {
                        println!("Packet data: {0:x?}", &self.buffer[0..samples_decoded])
                    }
                    Err(err) => {
                        dbg!(err);
                    }
                },
                Err(err) => {
                    dbg!(err);
                }
            }
        }
    }

    pub fn get_buffer_size(&self) -> usize {
        self.buffer.len()
    }
}
