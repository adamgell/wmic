# TLS test certificates

Self-signed certificates for the loopback TLS tests in `tests/wsman_transport.rs` and the
channel binding tests in `src/backend/wsman/transport/tls.rs`. The private keys are test
material only.

| File | Signature | Names | Use |
| --- | --- | --- | --- |
| `loopback-rsa-sha256.pem`, `.key` | sha256WithRSAEncryption | `localhost`, `127.0.0.1` | Trusted loopback server |
| `other-ecdsa-sha384.pem`, `.key` | ecdsa-with-SHA384 | `example.test` | SHA-384 binding; name mismatch |
| `legacy-rsa-sha1.pem` | sha1WithRSAEncryption | `legacy.test` | SHA-1 raised to SHA-256 |

The validity starts on 2019-01-01 and ends on 2119-01-01. The macOS trust store limits the
validity period of TLS server certificates issued after 2019-07-01, so a later start date makes
the trusted loopback test fail with `certificate is not standards compliant`.

Generated with OpenSSL 3.5:

```
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -not_before 20190101000000Z -not_after 21190101000000Z \
  -keyout loopback-rsa-sha256.key -out loopback-rsa-sha256.pem -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" -addext "extendedKeyUsage=serverAuth" \
  -addext "basicConstraints=critical,CA:TRUE"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-384 -nodes -sha384 -not_before 20190101000000Z \
  -not_after 21190101000000Z -keyout other-ecdsa-sha384.key -out other-ecdsa-sha384.pem \
  -subj "/CN=example.test" -addext "subjectAltName=DNS:example.test" \
  -addext "extendedKeyUsage=serverAuth" -addext "basicConstraints=critical,CA:TRUE"
openssl req -x509 -newkey rsa:2048 -nodes -sha1 -not_before 20190101000000Z -not_after 21190101000000Z \
  -keyout legacy-rsa-sha1.key -out legacy-rsa-sha1.pem -subj "/CN=legacy.test" \
  -addext "subjectAltName=DNS:legacy.test"
```

The expected digests in the tests come from .NET `HashAlgorithm` and `MD5` over the DER of
these files. Regenerating a certificate changes them.
