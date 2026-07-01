#!/usr/bin/env bash

# Gets the sampling rate of a given file using FFMPEG
# Source: https://stackoverflow.com/a/77973471
ffprobe -print_format json -show_format -select_streams a:0 -show_entries stream=sample_rate:format=0:stream_tags=0:format_tags=0 "$1"
