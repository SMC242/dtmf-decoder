# dtmf-decoder

This is a simple decoder for [DTMF signals](https://en.wikipedia.org/wiki/DTMF_signaling), the system used to encode the tones emitted when pressing the number pad during a phone call. Not to be confused with the Bad Bunny song.

## Motivation

- I was curious how phone bots understand which button I'm pressing as phone calls are audio-only
- I wanted to brush up on my signal processing as I have only covered it theoretically
  - See my [HTTP server implementation](https://github.com/SMC242/http-server) for another exploration of a field I had only covered theoretically

Originally inspired by [this Reddit post](https://www.reddit.com/r/explainlikeimfive/comments/ajsydg/eli5_how_do_phones_know_which_buttons_youve/)

## How it works

- DTMF signalling as specified by [ITU-T Recomendation Q.23](http://www.itu.int/rec/T-REC-Q.23/en) encodes each key on the 16-key keypad as the composition of two frequencies
  - One signal associated with the row, one with the column
- The signals are superimposed to create the tone, which is then played for at least 40ms
  - See [British Telephones' explanation of the signal constraints](https://www.britishtelephones.com/dtmf.htm)
  - You need to handle interrupted signals, which I think is why there is the <=23ms rule

## Plan

- [ ] Support a limited-duration audio file (.ogg, opus codec), returning a list of keys pressed
  - Take sampling rate as a parameter initially
  - [ ] Add support for reading the sampling rate from the file metadata
- [ ] Support real-time classification, updating the keys pressed every 80ms
- [ ] Wrap the program in a simple website so people can play with it without any setup

## Bells and whistles

- [ ] Support the [AMR-WB codec](https://en.wikipedia.org/wiki/Adaptive_Multi-Rate_Wideband) which is used for real cellular calls. Opus is optimised for the web
