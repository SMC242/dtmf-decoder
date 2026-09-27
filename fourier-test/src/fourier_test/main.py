import time
import typing

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
            "48000",
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
        time.sleep(1)


def find_tones(signal: Signal) -> npt.NDArray[np.int16]:
    """
    Reshape the array into tones
    """
    # A group is the region between the previous group and a jump in index
    labelled, _ = ndimage.label(signal)
    slices = ndimage.find_objects(labelled)
    groups = [signal[sl[0]] for sl in slices]
    print(f"groups = {groups[:5]}")
    print(f"shapes = {[g.shape for g in groups]}")

    play_tones(groups)
    return np.stack(groups, axis=1)


def denoise_tones(tones: npt.NDArray[np.int16]) -> npt.NDArray[np.int16]:
    """
    Remove tones with less than 10 samples. Expects shape (n,m)
    """
    MINIMUM_LENGTH = 10

    pass


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
    print(dtmf_tones)

    # freqs = fft(signal)
    # plot_signal(freqs)


if __name__ == "__main__":
    main()
