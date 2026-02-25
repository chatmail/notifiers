# Chatmail Notification Proxy

The Chatmail notification proxy is deployed as a central service
on https://notifications.delta.chat 

The notification proxy is a small Rust program 
that forwards "device tokens" to Apple and Google "Push Services"
that in turn wake up the clients
using [Chatmail core](https://github.com/chatmail/core/) on user's devices.

## Usage

### OpenPGP key

The OpenPGP key can be generated using [rsop](https://codeberg.org/heiko/rsop):

```console
$ rsop generate-key --profile rfc9580 > openpgp.privkey
$ rsop extract-cert < privkey > openpgp.pubkey
```

### APNS Certificates

The certificate file provided must be a `.p12` file. Instructions for how to create can be found [here](https://stackoverflow.com/a/28962937/1358405).


### FCM token

The FCM token can be retrieved in the Firebase console.

### Running

```console
$ cargo build --release
$ ./target/release/notifiers --certificate-file <file.p12> --password <password> --fcm-key-path <fcm.private> --openpgp-keyring-path <openpgp.privkey>
```

- `file.p12` is APNS certificate
- `password` is file.p12 password
- `fcm.private` is the FCM token
- `openpgp.privkey` is the generated OpenPGP key

### Registering devices

```console
$ curl -X POST -d '{ "token": "<device token>" }' http://localhost:9000/register
```

### Enabling metrics

To enable OpenMetrics (Prometheus) metrics endpoint,
run with `--metrics` argument,
e.g. `--metrics 127.0.0.1:9001`.
Metrics can then be retrieved with
`curl http://127.0.0.1:9001/metrics`.
