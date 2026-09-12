from scipy.fft import fft
import audiofile
import matplotlib.pyplot as plt
import numpy as np
import pathlib

FILE_PATH = "~/Downloads/dtmf-test.opus"


def plot_signal(sig: np.ndarray) -> None:
    _, ax = plt.subplots()
    ax.plot(sig)
    ax.set(xlabel="Time (s)", ylabel="Amplitude")
    ax.grid()
    plt.show()


def main() -> None:
    print("Start")
    expanded = pathlib.Path(FILE_PATH).expanduser()
    signal, sampling_rate = audiofile.read(expanded)
    print(f"{signal=}")
    plot_signal(signal)


if __name__ == "__main__":
    main()
