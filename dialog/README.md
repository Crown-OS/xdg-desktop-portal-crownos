# crownos-portal-dialog

The alert that xdg-desktop-portal-crownos, crownpositor and crown-contextd ask the user with.
It draws an overlay layer in the namespace `crownos-portal-dialog`. crownpositor gives no
synthetic input to that layer while root owns the installed executable
(`/usr/lib/crownos-portal-dialog`).

```text
crownos-portal-dialog --icon warning --title T --message M --cancel Deny \
    --choice grant=Allow --choice always="Always Allow" \
    --checkbox "Remember this choice" --initial true \
    --list "Show" --item "Window list" --item "Titles" --initial 0
```

## The answer

The dialog prints nothing. The answer is the exit status, as `crownos_config::consent_exit`
encodes it. That module is the one table that the dialog and every caller use.

| Status | Meaning |
|---|---|
| 1 | Declined: the cancel button, Escape, or the dialog closed |
| 2 | Misused: bad arguments, or a question with too many answers |
| 16–255 | Answered: `16 + choice × states + state` |
| any other, or a signal | Read as declined |

- `choice` is the index of the chosen `--choice` button, counted over all sections.
- `state` holds the value of each extra question, the first one lowest: a checkbox has 2 values
  (0 off, 1 on), a list has one value per `--item` (its index). `states` is the product of
  those counts.
- A question must fit: `choices × states ≤ 240`. A question that does not fit exits 2.

## Who it is

The compositor identifies the dialog when it connects, from its executable. Two frames after
start, when its surface shows, the dialog turns `PR_SET_DUMPABLE` off, so no other process of
the user can attach to it and answer.
