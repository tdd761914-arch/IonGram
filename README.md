# IonGram

Experimental **Rust + SQLite MTProto server** with API layers **229 and 231**.
MIT-licensed implementation inspired by [InvestorGram](https://github.com/opsdff/InvestorGram)
and selected behavior from [gramsrv](https://github.com/iamxvbaba/gramsrv).
Cryptography uses **grammers-crypto**, not tgcrypto. No Python runtime is used by the server.

## What works

- RSA_PAD and legacy RSA authorization-key exchange; MTProto 2.0 encryption,
  containers, acknowledgements, salts, gzip requests, reconnecting with stored auth keys.
- Full, Abridged, Intermediate, Padded Intermediate and Obfuscated TCP transports.
- Explicit API layer negotiation through `invokeWithLayer`: 229 or 231.
- Local login codes, persistent users, profiles, usernames and logout.
- Private messages and saved messages; groups, channels and supergroups;
  history, dialogs, forwarding, editing, deletion, replies and pinned messages.
- Channel administrators, membership, bans, public usernames, invite links,
  expiration, usage limits, revocation and join-request approval.
- Persistent per-user update cursors, per-channel PTS, live delivery and difference recovery.
- Multipart files, documents, JPEG/PNG photos and downloads with file references.
- TGS: gzip decoding with output limits, Lottie JSON validation, 512×512 dimensions,
  30/60 FPS, maximum three-second duration; original compressed bytes are preserved.
- Internal Stars balances and transaction history; atomic purchase forms with replay protection.
- Gift catalog imported from TGS, buying, displaying, hiding, conversion, upgrading,
  transfer, resale, upgrade previews and profile pins.
- Internal Premium expiry; administrative grants and buying Premium with internal Stars.
- Multiple collectible usernames on users/channels; activation, reordering,
  global case-insensitive uniqueness and `fragment.getCollectibleInfo`.

See [HANDLERS.md](HANDLERS.md) for the implemented RPC inventory and
[NOTICE](NOTICE) for source provenance. IonGram does not claim complete Telegram API parity.

## Build and run

Requirements: current stable Rust, a C compiler (bundled SQLite), Python 3 for the
one-time schema download. Python is also used by the optional interoperability test.

```sh
git clone https://github.com/tdd761914-arch/IonGram.git
cd IonGram
python3 tools/fetch_schemas.py
cargo build --release --locked
./target/release/iongram serve
```

Defaults: `127.0.0.1:4430`, storage in `./data`. The first start generates
`data/private.pem`, `data/public.pem` and prints the public-key fingerprint.

```sh
IONGRAM_DATA=/path/to/private-data \
IONGRAM_BIND=0.0.0.0:4430 \
IONGRAM_PUBLIC_HOST=your.server.ip \
IONGRAM_PORT=4430 \
RUST_LOG=iongram=info ./target/release/iongram serve
```

Clients must use **your server's DC address, public RSA key and fingerprint**.
An unmodified client pointed at Telegram's production DCs will not use IonGram.
The server advertises DC 2. Set `IONGRAM_PUBLIC_HOST` to an address the client can reach;
set `IONGRAM_PORT` if the advertised port differs from 4430.

`auth.sendCode` writes a short-lived code to the server log; `auth.signIn`
creates a local account on first successful login. There is no SMS provider,
Telegram-account import, passkey or 2FA service in this version. Codes are bound
to the auth key, expire after five minutes and allow five attempts.

## Local administration

**Stop the server before these commands.** An exclusive database lock prevents
an administration process and server from overwriting one another's state.
Use the same `IONGRAM_DATA` directory for every command.

```sh
./target/release/iongram key
./target/release/iongram users
./target/release/iongram credit 101 5000
./target/release/iongram premium 101 30
./target/release/iongram import-gift ./gift.tgs "My Gift" 100 1000
./target/release/iongram collectible user 101 RareName
./target/release/iongram collectible channel 102 ChannelName
./target/release/iongram serve
```

`credit USER_ID AMOUNT` appends a Stars ledger entry. `premium USER_ID DAYS`
extends the subscription from its current expiry, or now if expired.
`import-gift FILE TITLE PRICE SUPPLY` validates TGS and creates a limited catalog item;
upgrading costs 50 internal Stars. Premium gift pricing is 1,000 / 1,800 / 3,000
Stars for 3 / 6 / 12 periods of 30 days.

Collectible usernames created with the CLI are **local IonGram assets**.
Fragment RPC exposes their local metadata; no blockchain ownership is asserted.
A primary editable username remains distinct from additional collectible names.
Reordering accepts the full active username list, including the editable name;
omitted inactive names retain relative order at the end.

Stars, gifts and Premium here belong to IonGram. There is no connection to
Telegram balances/subscriptions, Fragment.com settlement, card payments or TON withdrawal.

## Data and protocol boundaries

SQLite uses WAL and FULL synchronous commits. The domain snapshot, Stars ledger,
balance projections, update journal and globally unique username registry commit
in one transaction. Updates are broadcast only after that commit. The server
keeps the domain snapshot in memory; this design is intended for small instances.
Keys and database live under a private data directory. Back up that whole directory,
including `files/`; stop the server or use a SQLite-aware backup.

Files are limited to 64 MiB when finalized, 512 KiB per upload part and 1 GiB
of staged parts per user. Remove abandoned staged uploads during maintenance.
TGS is validated and served, not rasterized or transcoded. JPEG/PNG dimensions
are read from headers; thumbnail generation, CDN redirects and video transcoding
are not implemented.

Unsupported methods return `METHOD_NOT_IMPLEMENTED`; they are not reported as successful.
This version does not implement bots, calls, stories, secret chats, web transports,
external payment providers, auctions, staking, gift crafting/collections, or full
multi-DC authorization. Some channel setting handlers persist configuration
without implementing every Telegram client-side behavior (for example slow-mode timing).
History filtering/pagination covers the basic ID/date/text case, not every Telegram filter.
The TCP server and key exchange are experimental and have not undergone a security audit.

Temporary authorization keys support `auth.bindTempAuthKey`: the server validates
MTProto v1 encrypted binding proofs, persists temp-to-permanent identities in
SQLite, and resolves login state through the permanent key. Expiry comes from
the DH handshake; expired temporary keys are rejected and reclaimed on later
key creation. This is basic PFS interoperability, not a full key-retention audit.

### Gift patterns and backgrounds

Stop the service before configuring a gift's upgrade attributes, and use the same
`IONGRAM_DATA` directory as the server:

```sh
iongram gift-style GIFT_ID assets/gifts/hexagon-pattern.tgs assets/gifts/hexagon-backgrounds.json
```

The supplied catalog has a hexagon pattern and black, red and green backgrounds.
Upgrading selects a background according to its rarity and persists the chosen
attributes with the owned gift. Transfers and resale preserve those attributes.
The command also fills in styles for previously upgraded gifts lacking a background.

## Validation

```sh
python3 tools/fetch_schemas.py
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

An independent wire test uses Telethon for RSA/DH, MTProto encryption and TCP framing,
with a small schema adapter for layers 229/231:

```sh
python3 -m pip install telethon==1.42.0
IONGRAM_DATA=wire-test-data IONGRAM_BIND=127.0.0.1:14430 ./target/release/iongram serve
# In another terminal, after the server is listening:
python3 tests/mtproto_smoke.py wire-test-data 14430
```

Use a fresh test data directory. CI runs both the Rust tests and the wire test.
The latter covers login, users, DM, groups, channels, invitations, TGS files,
Stars reads and Updates on four transports. Official Android/Desktop UI flows
have not been tested end to end.

## Schema versions

Schemas are downloaded from pinned commits and verified by SHA-256, not followed
from a moving `main` branch. Sources and checksums are in [schema/sources.json](schema/sources.json).
They are fetched separately to preserve their upstream provenance and licensing.
Changing layers requires reviewing changed constructors and updating compatibility tests.

## License

IonGram code: [MIT](LICENSE). Third-party dependencies and external schemas retain
their respective notices. See [NOTICE](NOTICE).
