# Automobilista 2 (Madness engine)

**Confidence:** Corroborated
**Checked:** 9 September 2026
**Writes settings:** not yet

## File

`{documents}\Automobilista 2\triplescreensettings.xml`

Corroborated across independent sources, including the well-known workaround of
configuring three screens in the Project CARS 2 demo and copying its
`triplescreensettings.xml` across — which only works because both are the same
engine writing the same file.

## Why nothing is written yet

The path is corroborated; **the element names inside it are not**. No source
consulted quoted the file's contents, and an XML element name that is almost
right is ignored silently, exactly like an INI key.

One further wrinkle worth recording: multiple sources report that AMS2's
in-game triple-screen UI has a bug that fails to save, which is why people edit
the file by hand. That makes this a genuinely valuable adapter — and also means
the file is the authority rather than the UI, so getting it right matters more
than usual.

## What would finish it

One real `triplescreensettings.xml` from a configured triple-screen install.
The **Inspect** button produces the element list with no values in it.

## Sources

- <https://steamcommunity.com/app/1066890/discussions/0/2137462524933762007/>
- <https://steamcommunity.com/app/1066890/discussions/0/5408241261727153978/>
- <https://steamcommunity.com/app/1066890/discussions/0/4631485178655551779/>
