# NoteSeq

A command line tool for playing note sequences.

## Usage

Playing a note sequence is as easy as:

`noteseq C E G`

To specify the octave of each note,
add the octave number at the end of each note name:

`noteseq E#5 D5 B4 G4 Bb4`

[!NOTE]

You don't need to specify the octave when playing in the 4th octave,
so the same sequence could be written as:

`noteseq E#5 D5 B G Bb`

To control the note values add another number to the end of each note name,
and separate the octave number and the note value with `:`.
The number

`noteseq D:8 E:8 F:8 G:8 E:4 C:8 D:4`

Sequences to use:

- The Lick - `noteseq D:8 E:8 F:8 G:8 E:4 C:8 D:4`
- Giant Steps `noteseq E#5 D5 B G Bb`
