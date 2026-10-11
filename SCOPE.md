# IonGram implementation scope

Rust MTProto server, MIT, based on InvestorGram handler behavior.

Requested domains: authorization, users, private messages, groups, channels,
files including validated TGS animations, gifts, internal Stars and Premium.
Cryptography: grammers-crypto 0.10.0; RSA key management: RustCrypto rsa.
API schema: TGScheme/Schema main_api.tl, layer 231, commit
c57fca59ec12f46272a6daad77bc256b532af8ca.
Reference: opsdff/InvestorGram a84bb655efed1042420e9912d942ae23aaab5542.

Internal Stars and Premium belong to this independent server. No connection
to Telegram's production balances, subscriptions or payment processing.

Additional API layer: 229, pinned TGScheme commit
fc7c7cc0d15461fa70263a4f539a6037a15231d5.
