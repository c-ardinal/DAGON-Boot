# Demo data

`demo-data.py` rebuilds a set of demo releases from the firmware in this
checkout: the hub app and the seg app for slots A and B, signed as 1.0.0 and
1.1.0, and releases with one bit of every image header flipped, to show a node
refusing them.

```sh
python samples/demo-data/demo-data.py --key <secret key file> --pubkey <public key file> [--out <dir>]
```

The script's help says what it writes and which folders it refuses.
