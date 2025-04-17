# NoteSeq

A command line tool for playing note sequences.

## Usage

Playing a note sequence is as easy as:

```
noteseq C E G
```

### Accidentals

Accidentals `#` an `b` can be added to any note.

```
noteseq C Eb G#
```

They can also be stacked and mixed.

```
noteseq Cbb E#### Gb#
```

### Octaves

To specify the octave of each note,
add the octave number at the end of each note name.

```
noteseq E#5 D5 B4 G4 Bb4
```

> [!TIP]
> You don't need to specify the octave when playing in the 4th octave,
> since this is the default octave.
> Therefore the same sequence could be written as:
>
> ```
> noteseq E#5 D5 B G Bb
> ```

### Note values

To control the note values another number is appended at the very end of the note name,
separated by a `:`.
This number represents the divisor in the fraction that is the note length.
E.g. 8 means an 1/8 note.

```
noteseq D:8 E:8 F:8 G:8 E:4 C:8 D:4
```

> [!TIP]
> You don't need to specify the note value for quarter notes,
> since this is the default value.
> The same sequence could be written as:
> ```
> noteseq D:8 E:8 F:8 G:8 E C:8 D
> ```

### Pauses

To insert pauses, omit the pitch from the note, but keep the note value.

```
noteseq E:4 G:8 :4 D#5:8 D5:2 :4 G:8 A#:4 B:2
```

### Tempo

Change the tempo (in BPMs) by using the option `-t` or `--tempo`.
The default tempo is 120 BPM.

```
noteseq --tempo 180 D:8 E:8 F:8 G:8 E:4 C:8 D:4
```

### Fermata

A fermata can be applied to the last note in the sequence with
`-f` or `--fermata`.

```
noteseq --fermata F4:8 A4:8 C5:8 F5:1
```

### Tuning

The tuning can be specified with `--tuning`.
The default tuning is 440 Hz.

```
noteseq --tuning 432 F4:8 A4:8 C5:8 F5:1
```

## Technical configuration

Noteseq will use your default output device for playing back sound.
This can be overridden by providing the option `-d` or `--device`,
specifying the device name.

```
noteseq --device "My audio device" C D E
```

Sample rate can be changed with `-s` or `--sample-rate`.
Default sample rate is 48000.

```
noteseq --sample-rate 44100 C D E
```
