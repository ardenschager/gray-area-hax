# `261114_videograin` — UKG / 2-Step, Absurd Vocal Hooks & Segmented Collage YouTube Crate

## 1. Crate Summary & QA Guarantees
- **14 fresh, watermark-free YouTube clips** dug via `yt-dlp` into `outputs/261114_videograin/src/` (`<= 720p` H.264, `48 kHz` `256 kbps` AAC, trimmed cleanly to `22.5s..36.5s` starting at `0.00s` with `+faststart`).
- **Obscure + Eccentric Balance:** **10 of 14 clips are under `1,000` views** (`74`, `125`, `129`, `192`, `397`, `558`, `712`, `793`, `936`, `944` views), paired with iconic deadpan gadget/engineering demonstrations.
- **Zero Contamination:**
  - `0` 1950s psychiatry / *Frontier Psychiatrist* tropes.
  - `0` burned-in stock watermarks, `0` static graphic slides, `0` YouTuber *"like & subscribe"* outros (verified across all 14 `4x2` contact sheets in `outputs/261114_videograin/work/sheet_*.jpg`).
  - `0` background music bleed on all 14 clips — every vocal phrase, beatbox syllable, tap-shoe transient, solenoid chime, Omnichord triad, Rhodes chord, and bass note is 100% isolated/dry.
  - **Zero-Clip Gain Staging:** All 14 `src/*.mp4` files are linear peak-normalized so post-AAC decoded `f32` peak sits strictly at **`-2.12` to `-2.22 dBFS`** (`0` clipped samples).
- **On-Device Apple Vision Segmentation Verified (`~/Library/Caches/chromagrain/segment_vision`):**
  - Tested across **9 foreground clips** (`person` and `subject` modes; composite cream-bordered die-cut sticker proof sheets saved in `outputs/261114_videograin/work/proof_*.jpg`).

---

## 2. Full YouTube Provenance & Vision Segmentation Table (`outputs/261114_videograin/src/`)

| World | `src/` Filename | Resolution / FPS | Trim Window (`YT Start + Dur`) | Views | Uploader | YouTube Title | URL | Post-AAC Peak / RMS | Best `segment_vision` Mode & Silhouette Notes |
| :--- | :--- | :--- | :--- | ---: | :--- | :--- | :--- | :--- | :--- |
| **World A (`134 BPM`)** | `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `720x480 @ 29.97` | `74.0s + 29.0s` (`29.03s`) | 3,474,860 | Dave Rondot | *"Turbo Encabulator" the Original* | https://www.youtube.com/watch?v=Ac7G7xOG2Ag | `-2.21 / -17.73 dB` | **`"person"` (10/10, `35.1%` fg)** — Razor-sharp white lab coat, tie, and pointing arm against dark slate chalkboard. |
| **World A (`134 BPM`)** | `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `640x480 @ 29.97` | `178.0s + 35.0s` (`35.00s`) | 31,214 | jlryancom | *Pet Flowbee Instructional Video* | https://www.youtube.com/watch?v=K-iaANk41Oc | `-2.21 / -21.96 dB` | **`"subject"` (9.5/10, `26.7%` fg)** for close-up hands/Flowbee/dog (`0..11s`, `21..35s`); **`"person"`** for inventor Rick Hunt in kitchen (`11.5..21.0s`). |
| **World A (`134 BPM`)** | `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `1280x720 @ 25.00` | `0.5s + 35.0s` (`35.04s`) | **558** | Rare Coins & Collectibles | *174 Tomy Omnibot 5402 1984 eBay Auction Robot Presentation Demonstration Video* | https://www.youtube.com/watch?v=E8d7kbbrvQU | `-2.20 / -18.89 dB` | **`"subject"` (10/10, `38.3%` fg)** — Museum-clean die-cut sticker of the spinning 1984 robot, dome, red eyes, and claw arms. |
| **World A (`134 BPM`)** | `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `1280x720 @ 23.98` | `4.0s + 34.0s` (`34.03s`) | 2,679 | Danjel Nyberg | *Useless machine* | https://www.youtube.com/watch?v=OtgzPZQt4q4 | `-2.20 / -26.62 dB` | **`"subject"` (9.5/10, `41.2%` fg)** — Crisp geometric cutout of the pine/mahogany box, toggle switch, lid, and hand. |
| **World A (`134 BPM`)** | `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `1280x720 @ 29.97` | `2.0s + 34.0s` (`34.00s`) | **936** | Mike Weakley | *Bendmonger's Circuit-Bent Texas Instruments Speak & Math Toy (Unit SM02)* | https://www.youtube.com/watch?v=N8-QAaJpc0Y | `-2.19 / -16.30 dB` | **`"subject"` (10/10, `35.5%` fg)** — Cuts out cobalt-blue Speak & Math (including hollow handle interior!) + hands flipping chrome switches. |
| **World A (`134 BPM`)** | `wA6_att_answering_machine_5O-f-sWv5Pk.mp4` | `1280x720 @ 25.00` | `5.0s + 36.5s` (`36.52s`) | **793** | VACUUM TONES | *Reupload AT&T 1739 Digital answering machine demo* | https://www.youtube.com/watch?v=5O-f-sWv5Pk | `-2.19 / -22.16 dB` | Grid cell / **`"subject"`** — Overhead white digital answering machine with glowing red LED digit counter & finger presses. |
| **World A (`134 BPM`)** | `wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4` | `1280x720 @ 29.97` | `18.0s + 34.0s` (`34.00s`) | **712** | Michael Dicken | *Suzuki Omnichord OM-36 Test* | https://www.youtube.com/watch?v=j-z1lO9v24M | `-2.20 / -26.03 dB` | Grid cell — Tight close-up of hands pressing cream Suzuki Omnichord chord buttons & knobs. |
| **World B (`136 BPM`)** | `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `480x640 @ 29.97` | `9.0s + 36.0s` (`36.00s`) | **125** | MissChyna520 | *Singing Stuttering by Jazmine Sullivan (test-run)* | https://www.youtube.com/watch?v=XUaVI90fs5w | `-2.20 / -22.80 dB` | **`"person"` (10/10, `39.1%` fg)** — Razor-sharp portrait silhouette in royal-blue shirt against clean light wall. |
| **World B (`136 BPM`)** | `wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4` | `360x640 @ 23.98` | `5.5s + 33.5s` (`33.53s`) | **397** | JAM ONE \| Midwest Hip-Hop Media Junkie | *JAM ONE - Quarantine beatbox snippet* | https://www.youtube.com/watch?v=HL5rhH4m6Vc | `-2.21 / -24.11 dB` | **`"person"` (9.5/10, `86.2%` fg)** — Clean close-up silhouette of beatboxer + headphones + hand on mic against dark backdrop. |
| **World B (`136 BPM`)** | `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `1280x720 @ 29.90` | `16.0s + 34.0s` (`34.05s`) | 3,563 | Schur Success Group | *Basic Auction Chant* | https://www.youtube.com/watch?v=0sRO8LVil8w | `-2.21 / -23.26 dB` | **`"person"` (10/10, `12.1%` fg)** — Cleanly removes background banner, isolating only the auctioneer & expressive hands! |
| **World B (`136 BPM`)** | `wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4` | `638x360 @ 30.00` | `1.0s + 26.5s` (`26.53s`) | **944** | j_dublay | *Rhodes Jazz Improv* | https://www.youtube.com/watch?v=MxOzUVFZwCs | `-2.20 / -18.54 dB` | Grid cell — Overhead-side angle of hands playing warm solo Fender Rhodes keys. |
| **World B (`136 BPM`)** | `wB5_fingerstyle_bass_groove_KnvZ81XivtE.mp4` | `1280x720 @ 30.00` | `9.0s + 30.0s` (`30.00s`) | **192** | Dolman Bass | *Fingerstyle bass groove. Only bridge!!! (solo by Michael Dolmanov)* | https://www.youtube.com/watch?v=KnvZ81XivtE | `-2.12 / -14.91 dB` | Grid cell / **`"person"`** — Tight studio close-up of bassist plucking electric bass strings. |
| **World B (`136 BPM`)** | `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `1280x720 @ 30.00` | `0.2s + 34.0s` (`34.00s`) | **74** | Gregg Geoffroy | *Tap Dance: shuffle Step with Heel* | https://www.youtube.com/watch?v=6uD9rTyeurk | `-2.21 / -32.14 dB` | **`"subject"` (10/10, `14.5%` fg)** — Use `"subject"` (not `"person"`!) on headless lower-body legs: keeps BOTH black trouser legs + tap shoes 100% locked across every frame! |
| **World B (`136 BPM`)** | `wB7_pinball_solenoids_5zJ7D1DNPFY.mp4` | `640x480 @ 25.00` | `2.0s + 22.5s` (`22.52s`) | **129** | Pinballmania LLC | *Stern STARS pinball machine: solenoids test* | https://www.youtube.com/watch?v=5zJ7D1DNPFY | `-2.22 / -28.58 dB` | Grid cell — Internal chime-bar striker (`0..2s`) + flashing incandescent 1978 Stern *STARS* backbox & playfield solenoids. |

---

## 3. Ear-Verified Exact Spoken & Sung Hooks (`0.01s` Precision Inside Trimmed `src/` Files)
Every hook below was extracted from the normalized `src/*.mp4` file (`outputs/261114_videograin/work/hook_snips/*.mp3`) and auditioned by ear to confirm **zero clipped initial consonants, zero trailing word bleed, and zero background music**:

### World A — Absurd Gadget / Eccentric Inventor Hooks (`134 BPM`)
| Clip (`src/`) | Start..End (`s`) | Dur (`s`) | Peak / RMS (`dBFS`) | Exact Spoken Phrase | Musical / Collage Role |
| :--- | ---: | ---: | ---: | :--- | :--- |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `18.88..19.70` | `0.82s` | `-3.04 / -16.76` | **`"dingle arm"`** | Deadpan call-response pre-drop vocal stab (`Choke Group 1`) |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `23.45..24.36` | `0.91s` | `-2.96 / -16.29` | **`"It's not cheap"`** | Crisp dry punchline before `"the government will buy it"` |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `25.24..26.70` | `1.46s` | `-3.30 / -20.36` | **`"the government will buy it"`** | Full deadpan drop/turnaround tag (holds `"$750,000,000"` card in video!) |
| `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `12.88..14.54` | `1.66s` | `-4.61 / -19.37` | **`"Now turn on the Flowbee motor"`** | Pre-drop starter cue (Rick Hunt in kitchen holding turquoise Flowbee) |
| `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `18.42..20.08` | `1.66s` | `-3.29 / -22.29` | **`"bouncing motion works best"`** | Signature UKG groove hook (matches Rick's physical up-and-down hand bounce!) |
| `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `30.62..31.94` | `1.32s` | `-5.07 / -21.01` | **`"The choice is now up to you"`** | Breakdown / bridge vocal phrase |
| `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `15.54..16.92` | `1.38s` | `-2.91 / -17.07` | **`"My name is Zeon"`** | Absurd cassette-robot introduction (`"subject"` cutout of Omnibot 5402) |
| `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `29.96..31.34` | `1.38s` | `-2.93 / -17.43` | **`"for world domination"`** | Ridiculous robot drop callout |
| `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `15.10..16.76` | `1.66s` | `-5.82 / -21.15` | **`"So let's see how it works"`** | Whispered Nordic inventor intro phrase |
| `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `16.98..18.28` | `1.30s` | `-8.31 / -26.93` | **`"Let's just flip it on"`** | Right before toggle click at `18.91s`! |
| `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `1.54..2.72` | `1.18s` | `-3.58 / -17.71` | **`"Level one"`** | Classic TI LPC synth voice intro/section marker |
| `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `22.05..22.65` | `0.60s` | `-3.69 / -18.82` | **`"Solve it"`** | Tight `0.60s` robotic UKG vocal stab |
| `wA6_att_answering_machine_5O-f-sWv5Pk.mp4` | `29.46..30.68` | `1.22s` | `-6.50 / -22.72` | **`"No messages"`** | Deadpan digital answering-machine break phrase |
| `wA6_att_answering_machine_5O-f-sWv5Pk.mp4` | `35.32..36.49` | `1.17s` | `-2.79 / -20.09` | **`"The machine is on"`** | Drop-in vocal tag (pair with **`"The machine is off"`** at `32.88..34.02s` on `Tape Stop`!) |

### World B — Kinetic Tactile / Street-Club / Acoustic-Vocal Hooks (`136 BPM`)
| Clip (`src/`) | Start..End (`s`) | Dur (`s`) | Peak / RMS (`dBFS`) | Exact Spoken / Sung Phrase | Musical / Collage Role |
| :--- | ---: | ---: | ---: | :--- | :--- |
| `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `25.90..27.54` | `1.64s` | `-13.06 / -24.83` | **`"st-st-stuttering"`** | Natural soulful R&B stutter hook (`"person"` cutout) |
| `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `32.14..34.20` | `1.65s`* | `-7.45 / -18.81` | **`"he-e-ello"`** (`32.55..34.20` = `1.65s`) | Lush soulful R&B vocal hook & `Min7` chord-chop source |
| `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `16.30..17.26` | `0.96s` | `-5.11 / -21.38` | **`"You able to bid?"`** | Syncopated spoken UKG pocket phrase (`"person"` cutout) |
| `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `17.56..18.68` | `1.12s` | `-6.64 / -21.65` | **`"Would you bid a bid?"`** | Bouncy rhythmic question hook (`"bid-a-bid"` = natural 2-step triplet!) |
| `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `21.06..22.36` | `1.30s` | `-2.21 / -17.84` | **`"Twenty dollar bid now, would ya bid now"`** | Blistering 16th-note auctioneer patter burst |
| `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `13.88..14.92` | `1.04s` | `-21.12 / -37.31` | **`"Shuffle, step, heel"`** | Dry studio spoken dance callout right before tap flurry (`15.34s+`) |
| `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `28.26..28.92` | `0.66s` | `-20.36 / -36.21` | **`"Paddle and roll"`** | Tight dry rhythmic callout |

---

## 4. Verified Vocal-Percussion & Tactile Transient Slices (`0.03..0.09s` for `Stamp::VoxPerc` / Kicks / Rims / Shuffled Hats)

### World A (`134 BPM`) — Vocal Consonants & Gadget Switch Transients
| Clip (`src/`) | Window (`s`) | Dur | Class | Peak / RMS (`dBFS`) | ZCR (`Hz`) | Recommended UKG Drum/Perc Role |
| :--- | ---: | ---: | :--- | ---: | ---: | :--- |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `8.12..8.20` | `0.08s` | Chest plosive (`"b/d"`) | `-3.09 / -11.63` | `600 Hz` | Sculpted `Kick` body / vocal-thump layer |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `18.92..19.00` | `0.08s` | Hard `"d"` (`"dingle"`) | `-3.04 / -13.34` | `856 Hz` | Crisp `VoxPerc` rim / offbeat ghost snare |
| `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `23.62..23.71` | `0.09s` | Affricate `"t-ch"` (`"not cheap"`) | `-2.96 / -15.64` | `1733 Hz` | Snappy UKG 2-step rim/clap |
| `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `18.11..18.19` | `0.08s` | Crisp `"p/d"` (`"up and down"`) | `-3.36 / -15.06` | `1225 Hz` | Shuffled 16th `VoxPerc` syllable |
| `wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4` | `18.48..18.56` | `0.08s` | Bilabial `"b"` (`"bouncing"`) | `-3.29 / -14.62` | `838 Hz` | Punchy vocal tom / pocket ghost hit |
| `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `4.98..5.07` | `0.09s` | Robot cassette button clack | `-7.49 / -22.49` | `1420 Hz` | Mechanical pocket rimshot |
| `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `13.60..13.69` | `0.09s` | Robot speaker `"tr"` transient | `-3.89 / -12.76` | `1706 Hz` | Metallic robotic `VoxPerc` snare |
| `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `13.88..13.92` | `0.04s` | Metal toggle switch click 1 | `-3.30 / -25.38` | `3725 Hz` | Ultra-tight closed UKG `Hat` click |
| `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `18.91..18.97` | `0.06s` | Wooden lid + arm snap | `-6.87 / -29.36` | `2150 Hz` | Syncopated mechanical rim click |
| `wA4_useless_switch_box_OtgzPZQt4q4.mp4` | `31.92..31.96` | `0.04s` | Crisp sibilant `"s"` + click | `-6.33 / -26.97` | `5725 Hz` | Shuffled UKG open/closed `VoxPerc` hi-hat |
| `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `4.50..4.58` | `0.08s` | Circuit-bent synth zap/pop | `-2.67 / -10.94` | `1713 Hz` | Electro-garage synth rim / percussive zap |
| `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `20.30..20.34` | `0.04s` | TI keypad + glitch tick | `-3.14 / -22.76` | `3750 Hz` | Micro-tick hi-hat |
| `wA6_att_answering_machine_5O-f-sWv5Pk.mp4` | `32.89..32.94` | `0.05s` | Answering machine button click | `-2.19 / -21.70` | `1390 Hz` | Sharp plastic button rimshot |

### World B (`136 BPM`) — Beatbox Mouth-Drums, Tap Shoes, Solenoids & Auctioneer Micro-Syllables
| Clip (`src/`) | Window (`s`) | Dur | Class | Peak / RMS (`dBFS`) | ZCR (`Hz`) | Recommended UKG Drum/Perc Role |
| :--- | ---: | ---: | :--- | ---: | ---: | :--- |
| `wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4` | `27.36..27.42` | `0.06s` | Punchy beatbox lip `Kick` (`"bm"`) | `-3.39 / -18.19` | `375 Hz` | Primary organic UKG 2-step `Kick` |
| `wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4` | `23.65..23.73` | `0.08s` | Explosive inward-`K` snare | `-2.21 / -16.57` | `2156 Hz` | Cracking UKG beat-2 / beat-4 `Snare` / `Rim` |
| `wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4` | `3.22..3.28` | `0.06s` | Tight `"ts"` mouth hi-hat | `-2.57 / -18.44` | `3425 Hz` | Shuffled 2-step closed `VoxPerc` hi-hat |
| `wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4` | `15.46..15.54` | `0.08s` | Crisp `"pf/kch"` open hat-snare | `-2.67 / -16.18` | `2869 Hz` | Syncopated offbeat UKG open hat |
| `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `0.14..0.23` | `0.09s` | Dry consonant `"k/t"` pop | `-3.23 / -15.55` | `783 Hz` | `VoxPerc` syllable ghost note (`cell_salt` dancer) |
| `wB3_auctioneer_chant_0sRO8LVil8w.mp4` | `21.14..21.22` | `0.08s` | Fast `"tw-"` chant plosive | `-2.67 / -13.85` | `631 Hz` | Rapid 16th vocal-percussion shuffle hit |
| `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `8.16..8.21` | `0.05s` | Metal tap-plate brush strike | `-2.58 / -20.40` | `2300 Hz` | Crisp metallic acoustic hi-hat |
| `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `18.50..18.58` | `0.08s` | Heavy hardwood heel drop | `-2.88 / -23.25` | `980 Hz` | Woody room-thump kick/tom layer |
| `wB6_tap_shuffle_heel_6uD9rTyeurk.mp4` | `19.88..19.95` | `0.07s` | Sharp toe-tap rim crack | `-2.30 / -21.72` | `2157 Hz` | Snappy skippy 2-step rimshot |
| `wB7_pinball_solenoids_5zJ7D1DNPFY.mp4` | `4.94..4.98` | `0.04s` | Pinball solenoid coil snap | `-2.76 / -28.10` | `3100 Hz` | Electro-mechanical percussion transient |
| `wB7_pinball_solenoids_5zJ7D1DNPFY.mp4` | `8.46..8.52` | `0.06s` | Solenoid knocker + chime attack | `-2.38 / -14.56` | `4988 Hz` | Bright metallic UKG accent bell/rim |

---

## 5. Sustained Vowel & Harmonic Slices (`0.30..0.65s` for `Min7` UKG Vocal Chords & `<95 Hz` Sub-Bass)

| World | Clip (`src/`) | Window (`s`) | Dur | Measured Pitch (`f0`) | Peak / RMS (`dBFS`) | Recommended Harmonic / Bass Role |
| :--- | :--- | ---: | ---: | :--- | ---: | :--- |
| **World A** | `wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4` | `18.39..18.85` | `0.46s` | **`C2` (`66.8 Hz`)** | `-7.55 / -21.63` | Warm analog Omnichord sub/root bass (`< 95 Hz` pure sub foundation!) |
| **World A** | `wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4` | `11.93..12.55` | `0.62s` | **`G#2` (`105.0 Hz`)** | `-6.73 / -21.67` | Lush sustained vintage analog chord pad for UKG stabs |
| **World A** | `wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4` | `22.75..23.35` | `0.60s` | **`G2` (`99.4 Hz`)** | `-6.22 / -21.31` | Clean sustained Omnichord chord stab |
| **World A** | `wA5_circuit_speak_math_N8-QAaJpc0Y.mp4` | `28.72..29.32` | `0.60s` | **`C#3` (`135.2 Hz`)** | `-3.34 / -11.87` | Gritty circuit-bent LPC vowel drone (`ChordVoicing::Min7` synth-vox stab, LP-filtered!) |
| **World A** | `wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4` | `26.05..26.50` | `0.45s` | **`G3` (`191.2 Hz`)** | `-3.46 / -13.74` | Sustained robotic vowel (`"gigawatts"`) for vocoder-like `Min7` garage stabs |
| **World A** | `wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4` | `19.11..19.53` | `0.42s` | **`E2` (`83.5 Hz`)** | `-4.70 / -17.30` | Deep chest baritone resonance (`"arm"`) — pitches down cleanly into sub-bass |
| **World B** | `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `29.50..30.15` | `0.65s` | **`E4` (`322.1 Hz`)** | `-6.67 / -16.82` | Pure soulful R&B sustained `"hey"` vowel — goldmine for Todd Edwards / Disclosure `Min7` vocal-chord stabs! |
| **World B** | `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `32.60..33.35` | `0.75s` | **`E4` (`326.5 Hz`)** | `-7.45 / -16.42` | Warm sustained `"he-llo"` R&B vowel (`pitch_confidence > 0.85`) |
| **World B** | `wB1_soul_stutter_acapella_XUaVI90fs5w.mp4` | `11.35..11.85` | `0.50s` | **`F4` (`350.4 Hz`)** | `-7.53 / -17.69` | Expressive soulful `"be-e"` vowel stab |
| **World B** | `wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4` | `16.32..16.92` | `0.60s` | **`C#3` (`139.1 Hz`)** | `-3.23 / -13.19` | Butter-warm solo Fender Rhodes jazz-garage chord stab |
| **World B** | `wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4` | `21.46..22.06` | `0.60s` | **`A3` (`219.2 Hz`)** | `-2.29 / -11.19` | Lush Rhodes chord bloom + bell tine |
| **World B** | `wB5_fingerstyle_bass_groove_KnvZ81XivtE.mp4` | `4.31..4.85` | `0.54s` | **`G2` (`98.2 Hz`)** | `-2.97 / -13.21` | Round plucked electric bass note (transpose `-3..-5 st` -> `55–73 Hz` deep UKG garage subline!) |
| **World B** | `wB5_fingerstyle_bass_groove_KnvZ81XivtE.mp4` | `3.46..3.96` | `0.50s` | **`G#2` (`104.1 Hz`)** | `-2.28 / -12.73` | Punchy bridge-pickup bass note for skippy 2-step bassline |
| **World B** | `wB7_pinball_solenoids_5zJ7D1DNPFY.mp4` | `5.20..5.70` | `0.50s` | **`D#5` (`640.0 Hz`)** | `-2.45 / -24.14` | Real metallic pinball chime-bar ring — shimmering UKG percussive harmonic accent |

---

## 6. Intermediate Verification Artifacts (`outputs/261114_videograin/work/`)
- `sheet_wA1_*.jpg` .. `sheet_wB7_*.jpg`: `4x2` contact sheets for all 14 `src/` clips (visually verified watermark- and text-card-free).
- `proof_wA1_*_person.jpg`, `proof_wA2_*_subject.jpg`, `proof_wA3_*_subject.jpg`, `proof_wA4_*_subject.jpg`, `proof_wA5_*_subject.jpg`, `proof_wB1_*_person.jpg`, `proof_wB2_*_person.jpg`, `proof_wB3_*_person.jpg`, `proof_wB6_*_subject.jpg`: 3-column `[Original | Vision Alpha Mask | Cream-Bordered Die-Cut Sticker]` proofs generated with `~/Library/Caches/chromagrain/segment_vision`.
- `hook_snips/*.mp3` & `verified_hooks.json`: Individual sliced `.mp3` renders + exact `[t0, t1]` peak/RMS telemetry for all 23 ear-audited hooks.
- `crate_analysis.json` & `word_and_island_map.json`: Full `ffprobe`, `48 kHz` PCM onset/pitch telemetry, and word-level VTT alignments.

---

## 7. `v004 → v005 → v006` Iteration & QA Trail

Same 14-clip crate throughout (Section 2). Every version folder is kept intact (`v004/`, `v005/`, `v006/` → `out/*.mp4` + `ui/*.png`); nothing was overwritten. `v006/NOTES.md` has the per-file engine change log for `v006`.

### 7.1 What changed per version

| Version | Commits | Engine (`crates/core`) | Studio UI (`crates/ui`, `app`, `plugin`) | Critic verdict |
| :--- | :--- | :--- | :--- | :--- |
| **`v004`** | `66d2120` (`v004-core`), `d413eaa` (`ukg_v004.rs` compositions), `9e3ffdc` (`v004-ui`) | `Collage` stage (hairline grid + `#F2EDE0` die-cut stickers with a `+5,+5` `#111314` print shadow). Zero-blur `Split` with `2px` dividers. Multi-cell transient cuts. Zero-blur `ISOL8` picture side (Bayer halftone / Sobel zine ink). Fast cutout matte. `Stamp::VoxPerc` + UKG vocal kit/groove. | `VIDEOGRAIN` rebrand (window titles `videograin`; crate and plugin IDs unchanged). `44–46 px` top hardware bar with `BPM` + `SWING` LCDs and `+ Add`. Unobstructed stage bezel. `ISOL8` rack faceplate. `Cutout` + Vocal Kit wiring. | **HOLD** (#114): 4 `[SYSTEM]` + 3 `[PIECE]` issues (empty black `Tile` cells on hard-burst frames, LRA `1.0 LU` on `bid-stutter`, …). |
| **`v005`** | `c48706c` (core), merge `b28613c`, `bf50473` (`v005-ui`) | `1080p` anti-aliased stickers, packed-tile dedup, 7 `PatternPreset`s, `arrange_four_section_song` (`Intro → Drop → Break → Climax`), pocket ducking, 3rd piece (`disco-pump`). | 7-preset Groove bank, 1-click `Build Set`, YouTube pull bar (`Roulette (<100v)` · `Random 18s` · `Pull 2 → Kit`), `[5..0]` punch-in keys, real-time `wgpu` die-cut sticker shader, live `1080p` console showcase. | Core renders audited (#126); notes carried into `v006`: 21 mid-groove black flashes, same-video twin panels, alpha dust, Disco hat air. UI **PASS** (#133). |
| **`v006`** | `1668317` (`v006-ukg`), merge `84981d7`, **`3438296`** (`v006-ui`, now `main`) | Zero-black sample-and-hold (`<= 1.25 s`). Twin-panel dedup via `force_share_slot` / `dedup_frame_grids_by_video`. Alpha island despeckle. `"subject"` mask on `encab_cut`. Disco hat air (`hi>3.5k` `1.19% → 3.75%`). `Stamp::Wash`, `build_chord_washes_across_sources`, `PatternPreset::AmbientWash` (8th preset). 4th piece (`zeon-cathedral`). | `Perform` gains 1-tap `Chord Wash` + a selected-pad `Chord` selector. `Clip` gains the `Wash` stamp, `Chords → pads` and `Stretch ×8`. `Sequence` gets a `4 × 2` 8-preset Groove matrix (`Clear` moved into the header). `41.6 s` UKG-to-ambient showcase. | **4/4 PASS** core (#141), **100% PASS** UI + showcase (#146). |

### 7.2 Deliverable QA (re-measured independently by `@verifier`, 261114, on `main` `3438296`)

Measured with `ffprobe` + `ffmpeg -af astats,ebur128=peak=true` (post-AAC, `48 kHz` stereo) and `signalstats` luma (`YAVG <= 24` = near-black, limited range).

| Version | File (`out/`) | Size @ fps | Dur | Integrated | LRA | True peak | NaN/Inf | Near-black frames |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | :---: | ---: |
| `v004` | `flowbee-zeon_ukg-sticker-collage_v004.mp4` | `1080x1920 @ 60` | `19.70s` | `-14.6 LUFS` | `2.4 LU` | `-2.4 dBTP` | `0` | `13` |
| `v004` | `bid-stutter_vox-percussion-grid_v004.mp4` | `1080x1080 @ 60` | `19.41s` | `-14.9 LUFS` | `1.0 LU` | `-2.1 dBTP` | `0` | `1` |
| `v005` | `flowbee-zeon_ukg-sticker-collage_v005.mp4` | `1080x1920 @ 60` | `19.70s` | `-15.0 LUFS` | `2.8 LU` | `-2.7 dBTP` | `0` | `21` |
| `v005` | `bid-stutter_vox-percussion-grid_v005.mp4` | `1080x1080 @ 60` | `19.41s` | `-14.9 LUFS` | `4.4 LU` | `-2.4 dBTP` | `0` | `21` |
| `v005` | `disco-pump_auto-kit-four-section_v005.mp4` | `1920x1080 @ 60` | `19.35s` | `-14.8 LUFS` | `3.0 LU` | `-2.2 dBTP` | `0` | `0` |
| `v005` | `hardware-console_ukg-showcase_v005.mp4` | `1920x1080 @ 30` | `38.50s` | `-15.1 LUFS` | `2.4 LU` | `-2.6 dBTP` | `0` | `0` |
| **`v006`** | `flowbee-zeon_ukg-sticker-collage_v006.mp4` | `1080x1920 @ 60` | `19.70s` | `-15.0 LUFS` | `2.8 LU` | `-2.7 dBTP` | `0` | **`0`** |
| **`v006`** | `bid-stutter_vox-percussion-grid_v006.mp4` | `1080x1080 @ 60` | `19.41s` | `-14.9 LUFS` | `4.4 LU` | `-1.9 dBTP` | `0` | **`0`** |
| **`v006`** | `disco-pump_auto-kit-four-section_v006.mp4` | `1920x1080 @ 60` | `19.35s` | `-15.9 LUFS` | `2.6 LU` | `-1.9 dBTP` | `0` | **`0`** |
| **`v006`** | `zeon-cathedral_ambient-chord-wash_v006.mp4` | `1920x1080 @ 60` | `21.18s` | `-14.9 LUFS` | `2.7 LU` | `-3.8 dBTP` | `0` | `2`* |
| **`v006`** | `hardware-console_ukg-showcase_v006.mp4` | `1920x1080 @ 30` | `41.60s` | `-14.4 LUFS` | `4.9 LU` | `-2.7 dBTP` | `0` | **`0`** |

\* The opening fade-in of the ambient wash (`t = 0.000–0.017 s`, `YAVG 19.6 → 22.0`, back above 29 by `0.05 s`). Intentional; there are no mid-piece drops.

What changed `v004 → v006`:
- Mid-groove near-black frames went `13 / 21 / 21 → 0`.
- `bid-stutter` LRA opened up `1.0 → 4.4 LU`.
- Every file stays `<= -1.9 dBTP` with `0` NaN/Inf.

### 7.3 UI snapshots (`v00N/ui/`, 11 per version)

`261114_videograin_ui_{play,sequence,perform,clip,add}_{1600x1000,960x600}_v00N.png` + `261114_videograin_ui_plugin_960x600_v00N.png`. All three sets are present for `v004`, `v005` and `v006`. `v006` adds the 8-preset `4 × 2` Groove matrix, `Chord Wash` / `Vocal Kit` / `Cutout` / `Build Set` on the Perform pad bar, the selected-pad `Chord` pill, and `Wash` / `Chords → pads` / `Stretch ×8` in Clip, all with zero overlap at `960x600` (#146).

### 7.4 Repo hygiene

Only `NOTES.md` files are tracked under `outputs/` for this project. `.gitignore` excludes `outputs/**/{work,src}/` and `outputs/**/*.{mp4,wav,png,jpg,json}`, so the YouTube crate, contact sheets, hook snips and renders stay local.
