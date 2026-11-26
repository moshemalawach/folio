# Folio: find, spelling, equations, sketches, audio and the reader (English).

## L4.5 The spelling menu
spell-looking = Looking for suggestions…
spell-no-suggestions = No suggestions
spell-add-to-dictionary = Add to dictionary
spell-ignore-all = Ignore all

## L4.8 Find on page
find-no-matches = No matches
find-position = { $current } of { $total }

## L4.8 Equation errors
math-error-too-deep = This equation is nested too deeply
math-error-missing-end = Something is missing at the end
math-error-stray-close = There is a {"}"} without a matching {"{"}
math-error-nothing-before = Put something before { $symbol }
math-error-missing-close = A { $symbol } is missing
math-error-needs-group = { $command } needs a {"{"}…{"}"} here

## L4.9 The spelling status
spell-names-two = { $first } and { $second }
spell-status-off = Spelling is not checked.
spell-status-loading = { $count ->
    [one] Loading the { $names } dictionary…
   *[other] Loading the { $names } dictionaries…
}
spell-status-ready = Checking spelling in { $names }.
spell-status-failed = { $count ->
    [one] Could not load the { $names } dictionary: { $error }
   *[other] Could not load the { $names } dictionaries: { $error }
}
spell-error-unreadable = the dictionary could not be read

## L4.9 Audio errors (they follow "…: " in toasts, so they start lowercase)
audio-error-not-playable = not a recording Folio can play
audio-error-no-microphone = no microphone was found
audio-error-mic-format = the microphone's sample format ({ $format }) is not supported
audio-error-already-stopped = the recording was already stopped
audio-error-stopped-unexpectedly = the recording stopped unexpectedly
audio-error-not-saved = the recording could not be saved to disk
audio-error-unreadable = this recording could not be read
audio-error-no-speakers = no speakers or headphones were found
audio-error-speaker-format = the speakers' sample format ({ $format }) is not supported
