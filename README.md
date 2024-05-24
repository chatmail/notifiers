# Delta Chat Notification Proxy 

The Delta Chat notification proxy is a small Rust program 
that forwards "device tokens" to Apple and Google "Push Services"
that wake up Delta Chat app on user's devices in turn. 

## Usage 

### Certificates

The certificate file provided must be a `.p12` file. Instructions for how to create can be found [here](https://stackoverflow.com/a/28962937/1358405).

### Running

```sh
$ cargo build --release
$ ./target/release/notifiers --certificate-file <file.p12> --password <password>
```

### Registering devices

```sh
$ curl -X POST -d '{ "token": "<device token>" }' http://localhost:9000/register
```

### Enabling metrics

To enable OpenMetrics (Prometheus) metrics endpoint,
run with `--metrics` argument,
e.g. `--metrics 127.0.0.1:9001`.
Metrics can then be retrieved with
`curl http://127.0.0.1:9001/metrics`.
