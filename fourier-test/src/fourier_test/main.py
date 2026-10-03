import time
import typing

from numpy.fft import fftfreq
from scipy.fft import fft
from scipy import signal as sci_signal
from scipy import ndimage
import matplotlib.pyplot as plt
import numpy as np
import numpy.typing as npt
import pathlib
import subprocess
import sounddevice as sd

FILE_PATH = "~/Downloads/dtmf-test.opus"
SAMPLING_RATE = 48_000

FFMPEG_PATH = "/usr/bin/ffmpeg"

Signal: typing.TypeAlias = npt.NDArray[np.int16]


def read_audio_raw(path: pathlib.Path) -> npt.NDArray[np.int16]:
    # Adapted from https://zulko.github.io/blog/2013/10/04/read-and-write-audio-files-in-python-using-ffmpeg/
    proc = subprocess.Popen(
        [
            FFMPEG_PATH,
            "-i",
            str(path),
            # Decode to int16 samples
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            # Sampling rate
            "-ar",
            str(SAMPLING_RATE),
            # Read as mono
            "-ac",
            "1",
            # Output to stdout
            "-",
        ],
        stdout=subprocess.PIPE,
        bufsize=10**8,
    )
    try:
        stdout, stderr = proc.communicate(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
        print(f"Subprocess didn't terminate on its own. Killed it")
        stdout, stderr = proc.communicate()
        print(f"stderr={stderr}")

    return np.frombuffer(stdout, dtype="int16")


def play_tones(tones: list[npt.NDArray[np.int16]]) -> None:
    """
    Play a sequence of tones to the sound device
    """

    print(f"Playing {len(tones)} tones")
    for i, g in enumerate(tones):
        print(f"Playing group {i} with shape {g.shape}")
        sd.play(g, blocking=True)
        time.sleep(0.2)


def plot_freqs(signal: npt.NDArray[np.int16]) -> None:
    """
    Do the Fourier transform on the signal and plot its frequencies
    """
    # From https://youtu.be/Y49XhqcZas4
    spectrum = fft(signal)
    freqs = fftfreq(len(signal), 1 / SAMPLING_RATE)

    pos_mask = freqs >= 0
    positive_freqs = freqs[pos_mask]
    magnitude = (2 / len(signal)) * np.abs(spectrum[pos_mask])

    plt.plot(positive_freqs, magnitude)
    plt.xlabel("Frequency (Hz)")
    plt.ylabel("Amplitude")
    plt.tight_layout()
    plt.show()


def find_tones(signal: Signal) -> npt.NDArray[np.int16]:
    """
    Reshape the array into tones. Resulting shape: (n_max, 9)
    Each tone will be padded at the end with zeroes
    """
    N_TONES = 9

    # A group is the region between the previous group and a jump in index
    labelled, _ = ndimage.label(signal)

    # Find the 9 tones
    # bincount counts the number of occurrences of each label
    sizes = np.bincount(labelled.ravel())
    sizes[0] = 0  # Ignore background
    keep = np.argsort(sizes)[-N_TONES:]

    # Remap to contiguous group numbers instead of sparse
    remap = np.zeros(sizes.size, dtype=np.int16)
    remap[keep] = np.arange(1, keep.size + 1)
    labelled = remap[labelled]

    slices = ndimage.find_objects(labelled)
    groups = [signal[sl[0]] for sl in slices]
    print(f"shapes = {[g.shape for g in groups]}")

    max_len = max([g.shape[0] for g in groups])
    padded = [np.pad(g, (0, max_len - len(g))) for g in groups]
    print(f"shapes = {[g.shape for g in padded]}")
    return np.stack(padded, axis=0)


def plot_signal(sig: np.ndarray) -> None:
    _, ax = plt.subplots()
    ax.plot(sig)
    ax.set(xlabel="Time (s)", ylabel="Amplitude")
    ax.grid()
    plt.show()


def main() -> None:
    np.set_printoptions(threshold=np.inf)

    # Try reading with FFMPEG
    expanded = pathlib.Path(FILE_PATH).expanduser()
    signal = read_audio_raw(expanded)
    print(signal.shape)
    # plot_signal(signal)

    # dtmf_tones = sci_signal.find_peaks(signal)
    dtmf_tones = find_tones(signal)
    print(f"{dtmf_tones.shape=}")

    plot_freqs(dtmf_tones[0])


if __name__ == "__main__":
    main()
