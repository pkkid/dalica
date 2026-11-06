//! The HTTPS transport against local servers on loopback (`security.md` §8.2): a download and a
//! ranged resume, redirects (an https → http one refused, a loop capped), a truncated body, a
//! stalled server, oversized headers, a proxy `CONNECT` used when set and skipped for a `NO_PROXY`
//! host, certificates that do not validate, and a handshake in TLS 1.2 and 1.3 with every cipher
//! suite, key exchange group and signature scheme the provider (`http::provider`) offers. No
//! internet: every server listens on 127.0.0.1 with a certificate signed by a test root made for
//! these tests (below; ECDSA P-256, and RSA-2048, ECDSA P-384 and Ed25519 for the handshakes, all
//! valid 2026–2125), and the transport trusts those roots only, or the bundled web roots.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_fetch::http::provider::{ALL_CIPHER_SUITES, provider};
use dalica_fetch::spec::hex;
use dalica_fetch::{Consent, Downloaded, FetchError, FileSpec, HttpsTransport, Options, Proxy, TransportError, USER_AGENT, download};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::sign::{CertifiedKey, SigningKey, SingleCertAndKey};
use rustls::{CipherSuite, NamedGroup, SignatureAlgorithm, SignatureScheme, SupportedProtocolVersion};
use sha2::{Digest, Sha256};

/// The test root (CN "Dalica test root").
const ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIBnDCCAUKgAwIBAgITeDFkTlJPkPQi+FhzWzD6h15STzAKBggqhkjOPQQDAjAb
MRkwFwYDVQQDDBBEYWxpY2EgdGVzdCByb290MCAXDTI2MDEwMTAwMDAwMFoYDzIx
MjUxMjMxMDAwMDAwWjAbMRkwFwYDVQQDDBBEYWxpY2EgdGVzdCByb290MFkwEwYH
KoZIzj0CAQYIKoZIzj0DAQcDQgAEcw97VXPOStu/ZNh74Gj11zGvmznTzgs44CsD
p+/Zq3CnKnhlxoy3j5H8MuwZOVaGoBsjZbn66qW4Q3ae12RPE6NjMGEwHQYDVR0O
BBYEFKdFHWy9a+yHOX4nUnlHBowbIpBDMB8GA1UdIwQYMBaAFKdFHWy9a+yHOX4n
UnlHBowbIpBDMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMAoGCCqG
SM49BAMCA0gAMEUCICw+qMn3k1FME4fAt6JufUVcHuykis67fzhy/oCaP/yGAiEA
yFbCe0MnTPgMPc2KX0wAYNxF+7t28lqrdYHXJcYdFQ4=
-----END CERTIFICATE-----
";
/// A server certificate for `localhost` and 127.0.0.1, signed by [`ROOT`].
const LOCAL: &str = "-----BEGIN CERTIFICATE-----
MIIBwjCCAWigAwIBAgIUC2W68T7mumfgOsy3b/DU2eYn9v0wCgYIKoZIzj0EAwIw
GzEZMBcGA1UEAwwQRGFsaWNhIHRlc3Qgcm9vdDAgFw0yNjAxMDEwMDAwMDBaGA8y
MTI1MTIzMTAwMDAwMFowEDEOMAwGA1UEAwwFbG9jYWwwWTATBgcqhkjOPQIBBggq
hkjOPQMBBwNCAATriJr8GXnd1mgUGf1afdhdxh6idqAA+CKwvEccdD4sSRYRhG3U
Ns0plVKQfgNF3I9JRCosaV0dHR4WF1Me/sP0o4GSMIGPMAwGA1UdEwEB/wQCMAAw
DgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMBoGA1UdEQQTMBGC
CWxvY2FsaG9zdIcEfwAAATAdBgNVHQ4EFgQUiiVHNrnCIKFYqn5kZvFKE5vxj4Qw
HwYDVR0jBBgwFoAUp0UdbL1r7Ic5fidSeUcGjBsikEMwCgYIKoZIzj0EAwIDSAAw
RQIhAPjv8qnTjQoZn8br2ZnabFO/QbiFAqCHOcS80iZLT2J3AiAZm9Yw9K/kQ18d
FH0bCQZQTNO1IRvTm4CsBN93+XLkyw==
-----END CERTIFICATE-----
";
const LOCAL_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/atjZegGyCAW8rBM
xypoX/ClzoknKjBWp6PYlW8lY9qhRANCAATriJr8GXnd1mgUGf1afdhdxh6idqAA
+CKwvEccdD4sSRYRhG3UNs0plVKQfgNF3I9JRCosaV0dHR4WF1Me/sP0
-----END PRIVATE KEY-----
";
/// A server certificate for `other.example` only, signed by [`ROOT`].
const OTHER: &str = "-----BEGIN CERTIFICATE-----
MIIBwDCCAWagAwIBAgIUC2W68T7mumfgOsy3b/DU2eYn9v4wCgYIKoZIzj0EAwIw
GzEZMBcGA1UEAwwQRGFsaWNhIHRlc3Qgcm9vdDAgFw0yNjAxMDEwMDAwMDBaGA8y
MTI1MTIzMTAwMDAwMFowEDEOMAwGA1UEAwwFb3RoZXIwWTATBgcqhkjOPQIBBggq
hkjOPQMBBwNCAATvipbutbisjqpJ5s1IcbrRcxZ2pMHzGe6zC3MW99SKrlzsnRH4
9SPX3+652LvytL5LBucJOBWpwm4PWfcc9L/ho4GQMIGNMAwGA1UdEwEB/wQCMAAw
DgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMBgGA1UdEQQRMA+C
DW90aGVyLmV4YW1wbGUwHQYDVR0OBBYEFKMk8q/TkfHB2MEJKcok+SfOHVzfMB8G
A1UdIwQYMBaAFKdFHWy9a+yHOX4nUnlHBowbIpBDMAoGCCqGSM49BAMCA0gAMEUC
IQC6tZ8UUDXJ2hQL0DrysydnaRSS4tKT4jVGzGQUtIPBHgIgQCu23Vwsp4eDS5uA
02koSfuLyO6aggexdU6jELk+wcc=
-----END CERTIFICATE-----
";
const OTHER_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgLrVYvLfMX8OF7+Fy
AT50e778V3e29116loLbx9GLqSmhRANCAATvipbutbisjqpJ5s1IcbrRcxZ2pMHz
Ge6zC3MW99SKrlzsnRH49SPX3+652LvytL5LBucJOBWpwm4PWfcc9L/h
-----END PRIVATE KEY-----
";

/// Roots, `localhost` and 127.0.0.1 certificates and their keys of the other key types, for the
/// handshakes (an RSA root signs with PKCS#1 SHA-256, P-384 with ECDSA SHA-384, Ed25519 with itself).
const RSA_ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIDEDCCAfigAwIBAgIUEFoQI9ah6ToEZhG8ZHNBXOQrGe8wDQYJKoZIhvcNAQEL
BQAwHzEdMBsGA1UEAwwURGFsaWNhIHRlc3Qgcm9vdCByc2EwIBcNMjYwMTAxMDAw
MDAwWhgPMjEyNTEyMzEwMDAwMDBaMB8xHTAbBgNVBAMMFERhbGljYSB0ZXN0IHJv
b3QgcnNhMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1U/E0oXi+UMu
T5qJGBigU/F17+g06iVEloKfsiJO0j9gF0/FdamzMQfkOF2C1BRzYakEndgwa5XL
XMB+m3sfUF7OB0/ofm6xk6dsHmnj4cR6k+WKugTR8fqONPWJAYZMqYabifwzkBKX
U19eAUk87oRiovHwpGFgplkggVjx4N4PmF72xTgI6HhMiOzQ7a+ufkivDFEPRXzn
5Xn87tdcR3P4mO12LY9iim9da1x+wFhuTD8GFiKyUA2GHcW9SonmlpWXO01HpKUv
WWhFU24XK6C67FIXX80M0fod+oqO+YtgxGNZgQVdTBMpPqaIkCApd3LNdnqpyc+z
Jw5VbWaySQIDAQABo0IwQDAdBgNVHQ4EFgQUw8ubERMTsfWwOh+Aqrh9nYVM0cYw
DwYDVR0TAQH/BAUwAwEB/zAOBgNVHQ8BAf8EBAMCAQYwDQYJKoZIhvcNAQELBQAD
ggEBANBK3z3zxf9QSHFgbzZXOrFNx2Ck1aOlK8S8aRTzGkHMdrrxh7HjD+8t1uhP
QphSW6OwMBzkayliuyYfbp1rIyh4GFpvcTxHodxMetFrlChxDt3VLkKHZeizuRV7
ZqqvVnTvBM5uKxEpHGOq0XfsRrHi9I+lMt33N88zMnzrRr1T5kb7hh8v0VZLKYLL
kvUR1TNeFrc/fai1ddjooBG1mS/k2b1NBdpCA55wjM2uFawxtBNvx2NcMqypdngn
1eGzNbI6WbDIMbpBgQqrru+vKaGI1fSeY+Q0iyOnCmV33f3qUObgyJqCexgHrk6T
q3PN+J+hxxywLkTzkgRu433KwcE=
-----END CERTIFICATE-----
";
const RSA_LOCAL: &str = "-----BEGIN CERTIFICATE-----
MIIDDTCCAfWgAwIBAgIRANtqEkXvnr4QWQJpNWAe3/MwDQYJKoZIhvcNAQELBQAw
HzEdMBsGA1UEAwwURGFsaWNhIHRlc3Qgcm9vdCByc2EwIBcNMjYwMTAxMDAwMDAw
WhgPMjEyNTEyMzEwMDAwMDBaMBAxDjAMBgNVBAMMBWxvY2FsMIIBIjANBgkqhkiG
9w0BAQEFAAOCAQ8AMIIBCgKCAQEArqA+vfaIvd3Wu7hWTsYvoqjNuhxDA+PLBHuI
JCkwYPppJsb9xAJ7BanMD31b0jnVm62h+cXlmZ1kCECY8bfw4a5GJT61nmi0weqm
qacwiLfI4n2Ens8i/6TvcxzB3odTMy9aRwQLEMj6tfGb7WzHmPK+XHLxYI141GI0
lwa8beHGCyA0yYg4Gxg6DPNG95BgGRLwOBHSF5XA/blsDjUhiOKLSYkBy77r4WOg
TkNjnBI9GT6xC515TGVfpgYYqSySYKnjsHMB/AoFDg/sc27u5apQaDwP7PCmFFI8
7eKR/8ONy4fQwQ7D379emTL9sPExCvLYU0STEbb1mmbtscePgQIDAQABo1EwTzAM
BgNVHRMBAf8EAjAAMA4GA1UdDwEB/wQEAwIHgDATBgNVHSUEDDAKBggrBgEFBQcD
ATAaBgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwDQYJKoZIhvcNAQELBQADggEB
AM405wKL5rBHtlF6DE/korjOozWPhoI7Xnrv3FpVhgXclNWxMdI998XDq576Uq7p
GXmFEySpEhh61T3syaV35s98IJhJq5jfouNThTJDTqsTQNnZrHfgeYvo1Rh99NER
Eo2IzaN3OX2eOiytkl1QpLSUq1F4JNCpsHLvedS5uYDdxvdpR1MP5KtySAOTfD53
ZxKga5EjVMFIrX5VoU1H7kMtWMMCO5Q3YnzY+qv1Kie7KBZZLs5TJsXihU4VumHo
Xzgk7fIPxYVlR+HMr4ANZS7eEHFwOBa+5rxmQ715EMh5JoDVJTubRLIfbzbLbZFv
len2eK+5iVLKbzh1erVnm+k=
-----END CERTIFICATE-----
";
const RSA_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCuoD699oi93da7
uFZOxi+iqM26HEMD48sEe4gkKTBg+mkmxv3EAnsFqcwPfVvSOdWbraH5xeWZnWQI
QJjxt/DhrkYlPrWeaLTB6qappzCIt8jifYSezyL/pO9zHMHeh1MzL1pHBAsQyPq1
8ZvtbMeY8r5ccvFgjXjUYjSXBrxt4cYLIDTJiDgbGDoM80b3kGAZEvA4EdIXlcD9
uWwONSGI4otJiQHLvuvhY6BOQ2OcEj0ZPrELnXlMZV+mBhipLJJgqeOwcwH8CgUO
D+xzbu7lqlBoPA/s8KYUUjzt4pH/w43Lh9DBDsPfv16ZMv2w8TEK8thTRJMRtvWa
Zu2xx4+BAgMBAAECggEACnx9+AtXb8tkCzbJHAbjsmlfnUb4MUIbHWRLqxw8Fv7C
lL0LXRtnBtKI+f8lpeyqKqSVHvomzQA6ExLJ/G32zZ6ZAMCQyHfyLtjn9DUhgXxD
2HIAJkc7C038wRVfgPzwXpj/21bfUT04bPeazPmkzMb67nNKkmk8cciJ7u3NH+3+
4Gx/Q2Bnaan8gCZUyZuBGeAvKHzRFVxH4ntWLAiqMDInWgKsOQPCTNXCdVmQQI7f
Y2v+1YH13RJq4cAzZxAxq8gdJ/xQFX4UhWmtYhpAjZ20O1abKl2TdzRBbUqjss8C
hkmrg3+tuqYYa74MXhiXgcy+7GRsY5GAPYjEgR6UIQKBgQDpyMyhbYj7adFCFTPi
+Kd7ychKY8R8sawzY3QjKsepdpzxcrDrw4meKTm75nKn9pIwA1taMBF0hWc5pdLi
5TEbH9kU+R9Y0VLbSL4LqDt6l7DlIuHFbKYxWzKTQN98GvDm1HivFzjB4jj59QSd
lqRLzkXsds2SfEfiISXeBM1fGQKBgQC/OFEwbZ1JgLE3HxSkK6T3msMmD+N2PHwo
Tevdb55fEbJcEIp3sNlFfA33lnyNlX1z4EuYlt5hA7rsY5bjM4julkV6DGms4yan
91Rc/IgjS3QDe4yKgtg1YTpsAaLpTmI529LPwYBS0ygyoRfk013/Ii8ifkizoaNB
+ziq23MIqQKBgEUs5V/eyWmbJhAntfKzINvAGUPpwXokGJAcHYFKZ0n9jvLivPcG
JPJnwG2ZAtQwYZFTbPSCxKd10dV8+6mnp6AfDFzoWh4t0haQr6tDXFYqDG3ApI2G
WKbCFXT8/SP71Mx4kj084c584d8UYQlASd0kbAxMGwf9PkfrBwlq4loBAoGAfocg
SeAuNGXAXY3mVZ8/gEtal8PpqeHp8a1lxddGvvdxwrN7PCm8CttETrC35+Oj6Fnh
C63ApgcW17YqMFp4YA71GAZAopKHkJr6qrC60EfsEyXcYAamU+PrCW6nwqMIIerP
+oK6i5OHRMENk289AkY3VCalqp7o7Q7zdzLQUkkCgYA/IZdNJ6+PeosT75QiYzR4
bL8Ls+bSSkmawMCgCR33ptReQ8eld11fG5CkYJ1F+tFrxwoycBf6m912yy5ZKPhM
qs3MJBH4OGl1bzZTbBGx4UYc7x80blQBhrlKXEpnHpQpFqPCUIdnpWppbviB/RIy
JHewxr/BSqs7EFHe+1Jjqg==
-----END PRIVATE KEY-----
";
const P384_ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIBwzCCAUmgAwIBAgIUDqd5H3zn8/BbRWeTWKWAjRcEZIswCgYIKoZIzj0EAwMw
IDEeMBwGA1UEAwwVRGFsaWNhIHRlc3Qgcm9vdCBwMzg0MCAXDTI2MDEwMTAwMDAw
MFoYDzIxMjUxMjMxMDAwMDAwWjAgMR4wHAYDVQQDDBVEYWxpY2EgdGVzdCByb290
IHAzODQwdjAQBgcqhkjOPQIBBgUrgQQAIgNiAASV3xwaF83KO4jdYuuQFBTmGbZz
5o0kj2c1bq093Cn5hCgeVYTSFwYk/D7OQwiHoFZJYjE+7oGlQCUYohHzvY7hH/4R
2zF78gCy+CfbTmNs/M0CauRGKgmbgp6hXFy7l7WjQjBAMB0GA1UdDgQWBBQB9Va1
8tTQ1Z6jRcD0Crtjyy/e0zAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIB
BjAKBggqhkjOPQQDAwNoADBlAjEAnko3y8URiiaqfoxyf29kGm00S4xQY4iMObkl
P4duTG5sR4fiD1+v6db5TjumCxW7AjAbZiFSazkZrad3Vy88t9Z2D9Z3el8GULLU
PuQApLEv7Kpv9UBmgu3dsIns7FOh0sw=
-----END CERTIFICATE-----
";
const P384_LOCAL: &str = "-----BEGIN CERTIFICATE-----
MIIBvzCCAUSgAwIBAgIQKwsIwC8OC61RWviOAMIjIDAKBggqhkjOPQQDAzAgMR4w
HAYDVQQDDBVEYWxpY2EgdGVzdCByb290IHAzODQwIBcNMjYwMTAxMDAwMDAwWhgP
MjEyNTEyMzEwMDAwMDBaMBAxDjAMBgNVBAMMBWxvY2FsMHYwEAYHKoZIzj0CAQYF
K4EEACIDYgAEjdckbVkup+ljFFFhf67K81uEeGTjoQtl9yMqh0iuoQ6lKCdQglWq
tfh2VtoJKQLZS3G+afAbVvQ7De/X+OWVpa1NwTeEejZ726P1BZMk0s3A0zZT4w+2
A1D4RbQoxZPro1EwTzAMBgNVHRMBAf8EAjAAMA4GA1UdDwEB/wQEAwIHgDATBgNV
HSUEDDAKBggrBgEFBQcDATAaBgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwCgYI
KoZIzj0EAwMDaQAwZgIxANOUx5S36zONXtxUI0vwaleGjg0cwoVXr7GCJuaw9tfG
9VVOZFlqYtEecYupXPJC/QIxAN0Sy8JDgAb8dHICikL0P8XMslCr0eqpqNP5MJHq
VFTwqrA7yDbnUPyAH4UYFWCJaQ==
-----END CERTIFICATE-----
";
const P384_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDB6g6GJjwZDxAR0itTO
o39AEMZqsbPk7bwLgk0hQ7zaDjeW3wGZwGf5nHP1b7K8LGGhZANiAASN1yRtWS6n
6WMUUWF/rsrzW4R4ZOOhC2X3IyqHSK6hDqUoJ1CCVaq1+HZW2gkpAtlLcb5p8BtW
9DsN79f45ZWlrU3BN4R6Nnvbo/UFkyTSzcDTNlPjD7YDUPhFtCjFk+s=
-----END PRIVATE KEY-----
";
const ED25519_ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIBSzCB/qADAgECAhQoSQykr6bleS0mt+bjBgN/UWqf6DAFBgMrZXAwIzEhMB8G
A1UEAwwYRGFsaWNhIHRlc3Qgcm9vdCBlZDI1NTE5MCAXDTI2MDEwMTAwMDAwMFoY
DzIxMjUxMjMxMDAwMDAwWjAjMSEwHwYDVQQDDBhEYWxpY2EgdGVzdCByb290IGVk
MjU1MTkwKjAFBgMrZXADIQA/ZJLEDTpeIX+Nlqctu832hNjAfHCY2iHI/rwvtptE
OKNCMEAwHQYDVR0OBBYEFDeA8Xq8CluYGGTUIa9YOJ/D7yJlMA8GA1UdEwEB/wQF
MAMBAf8wDgYDVR0PAQH/BAQDAgEGMAUGAytlcANBAH8EuUFkLt6JfqHaGF3P0gUB
hvAiRkdU38mVAsM9UurTvi/fuZTQMD/B0HpH7bYfuYRGzbZyJpAlAiU0R2Dp1gQ=
-----END CERTIFICATE-----
";
const ED25519_LOCAL: &str = "-----BEGIN CERTIFICATE-----
MIIBRDCB96ADAgECAhEAhfUdI6NqKMZagFx15VOzUTAFBgMrZXAwIzEhMB8GA1UE
AwwYRGFsaWNhIHRlc3Qgcm9vdCBlZDI1NTE5MCAXDTI2MDEwMTAwMDAwMFoYDzIx
MjUxMjMxMDAwMDAwWjAQMQ4wDAYDVQQDDAVsb2NhbDAqMAUGAytlcAMhAJ+fjGgN
wK500y0v5DiaXmLI3I1wt3YcL/nrO5FHQ/hNo1EwTzAMBgNVHRMBAf8EAjAAMA4G
A1UdDwEB/wQEAwIHgDATBgNVHSUEDDAKBggrBgEFBQcDATAaBgNVHREEEzARggls
b2NhbGhvc3SHBH8AAAEwBQYDK2VwA0EA+GXCdtqgrynlifaJwtUZiHf1S9AF7WRB
piKY01PKtiU42Mzchj9GTWx2H2lXfQuKUlic9RosE8QXYm7Eu6e7DQ==
-----END CERTIFICATE-----
";
const ED25519_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIHrSp45DGqFgmyduU4+bmWncxQm7PCAPLGNdYNa5rR2B
-----END PRIVATE KEY-----
";

fn root() -> Vec<Vec<u8>> {
    vec![CertificateDer::from_pem_slice(ROOT.as_bytes()).unwrap().to_vec()]
}

/// The transport trusting the test root, through `proxy`.
fn transport(proxy: Proxy) -> HttpsTransport {
    HttpsTransport::with_roots(proxy, &root()).unwrap()
}

/// What a test server does with one request.
enum Answer {
    /// Sends the bytes, then closes the TLS session properly.
    Send(Vec<u8>),
    /// Sends the bytes, then drops the connection without ending the TLS session.
    Cut(Vec<u8>),
    /// Sends the bytes, then holds the connection open, silent.
    Stall(Vec<u8>),
}

/// A TLS server on 127.0.0.1 answering every connection with `answer(index, request head)`; it
/// records each request head it read.
struct Server {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(cert: &str, key: &str, answer: impl Fn(usize, &str) -> Answer + Send + Sync + 'static) -> Server {
        let chain = vec![CertificateDer::from_pem_slice(cert.as_bytes()).unwrap()];
        let key = PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap();
        Server::with_config(Arc::new(config), answer)
    }

    fn with_config(config: Arc<rustls::ServerConfig>, answer: impl Fn(usize, &str) -> Answer + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let answer = Arc::new(answer);
        let seen = Arc::clone(&heads);
        std::thread::spawn(move || {
            for (i, sock) in listener.incoming().enumerate() {
                let Ok(sock) = sock else { continue };
                let (config, answer, seen) = (Arc::clone(&config), Arc::clone(&answer), Arc::clone(&seen));
                std::thread::spawn(move || serve_one(sock, config, i, answer.as_ref(), &seen));
            }
        });
        Server { port, heads }
    }

    fn url(&self, path: &str) -> String {
        format!("https://127.0.0.1:{}{path}", self.port)
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

fn serve_one(sock: TcpStream, config: Arc<rustls::ServerConfig>, i: usize, answer: &dyn Fn(usize, &str) -> Answer, seen: &Mutex<Vec<String>>) {
    sock.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let conn = rustls::ServerConnection::new(config).unwrap();
    let mut tls = rustls::StreamOwned::new(conn, sock);
    // a handshake the client refuses ends here, before any request
    let Some(head) = read_head(&mut tls) else { return };
    seen.lock().unwrap().push(head.clone());
    let (bytes, end) = match answer(i, &head) {
        Answer::Send(b) => (b, 0),
        Answer::Cut(b) => (b, 1),
        Answer::Stall(b) => (b, 2),
    };
    if tls.write_all(&bytes).and_then(|()| tls.flush()).is_err() {
        return;
    }
    match end {
        0 => {
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        1 => {
            let _ = tls.sock.shutdown(Shutdown::Both);
        }
        _ => std::thread::sleep(Duration::from_secs(20)),
    }
}

/// Reads a request head (up to the empty line) from `r`.
fn read_head(r: &mut impl Read) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match r.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// An HTTP proxy on 127.0.0.1: answers `CONNECT` with `status` and, on 200, tunnels to the
/// address asked for; records each request head.
struct ProxyServer {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl ProxyServer {
    fn start(status: u16) -> ProxyServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&heads);
        std::thread::spawn(move || {
            for sock in listener.incoming() {
                let Ok(mut sock) = sock else { continue };
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || {
                    let Some(head) = read_head(&mut sock) else { return };
                    seen.lock().unwrap().push(head.clone());
                    let target = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                    if status != 200 {
                        let _ = sock.write_all(format!("HTTP/1.1 {status} No\r\nContent-Length: 0\r\n\r\n").as_bytes());
                        return;
                    }
                    let Ok(server) = TcpStream::connect(&target) else { return };
                    let _ = sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                    let (mut a, mut b) = (sock.try_clone().unwrap(), server.try_clone().unwrap());
                    let (mut sock, mut server) = (sock, server);
                    std::thread::spawn(move || {
                        let _ = std::io::copy(&mut a, &mut server);
                    });
                    let _ = std::io::copy(&mut b, &mut sock);
                });
            }
        });
        ProxyServer { port, heads }
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

/// Seeded bytes standing in for a model file.
fn file(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(29) ^ ((i >> 8) as u8)).collect()
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    r.extend_from_slice(body);
    r
}

fn get(t: &HttpsTransport, url: &str, dir: &Path, opts: &Options) -> Result<Downloaded, FetchError> {
    let spec = FileSpec::new(url, "file.bin", 1 << 20, None, None).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    download(&spec, &consent, dir, t, opts, &AtomicBool::new(false), &mut |_| {})
}

fn once() -> Options {
    Options { connect_timeout: Duration::from_secs(5), stall_timeout: Duration::from_secs(5), attempts: 1 }
}

#[test]
fn a_download_is_cut_and_resumes_with_a_range() {
    let bytes = file(200_000);
    let half = 70_000;
    let body = bytes.clone();
    let server = Server::start(LOCAL, LOCAL_KEY, move |i, head| {
        if i == 0 {
            // announces the whole file, sends part of it, drops the connection
            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
            r.extend_from_slice(&body[..half]);
            return Answer::Cut(r);
        }
        let from: usize = head.split("Range: bytes=").nth(1).and_then(|r| r.split('-').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
        let rest = &body[from..];
        let mut r = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\n\r\n",
            rest.len(),
            body.len() - 1,
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(rest);
        Answer::Send(r)
    });
    let sha = hex(&Sha256::digest(&bytes));
    let spec = FileSpec::new(&server.url("/models/face.onnx"), "face.onnx", 1 << 20, Some(bytes.len() as u64), Some(&sha)).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let opts = Options { attempts: 2, ..once() };
    let done = download(&spec, &consent, tmp.path(), &transport(Proxy::direct()), &opts, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), bytes);
    assert!(done.resumed_from > 0 && done.resumed_from <= half as u64, "{done:?}");
    let heads = server.heads();
    assert_eq!(heads.len(), 2);
    assert!(!heads[0].contains("Range:"), "{}", heads[0]);
    assert!(heads[1].contains(&format!("Range: bytes={}-\r\n", done.resumed_from)), "{}", heads[1]);
    // exactly the headers fetch built, plus Host and Connection
    let names: Vec<&str> = heads[1].lines().skip(1).filter(|l| !l.is_empty()).map(|l| l.split(':').next().unwrap()).collect();
    assert_eq!(names, ["Host", "User-Agent", "Accept", "Accept-Encoding", "Range", "Connection"]);
    assert!(heads[1].starts_with("GET /models/face.onnx HTTP/1.1\r\n"));
    assert!(heads[1].contains(&format!("Host: 127.0.0.1:{}\r\n", server.port)));
    assert!(heads[1].contains(&format!("User-Agent: {USER_AGENT}\r\n")));
}

#[test]
fn a_truncated_body_fails() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| {
        let mut r = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n".to_vec();
        r.extend_from_slice(&[7u8; 400]);
        Answer::Cut(r)
    });
    let tmp = tempfile::tempdir().unwrap();
    let r = get(&transport(Proxy::direct()), &server.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Io(m))) if m.contains("before the end")), "{r:?}");
    // a chunked body cut in a chunk fails too
    let chunked = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Cut(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nabc".to_vec()));
    let r = get(&transport(Proxy::direct()), &chunked.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Io(_)))), "{r:?}");
}

#[test]
fn redirects_are_followed_capped_and_never_to_http() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, head| {
        if head.starts_with("GET /start ") {
            Answer::Send(b"HTTP/1.1 302 Found\r\nLocation: /dir/real?sig=1\r\nContent-Length: 4\r\n\r\nskip".to_vec())
        } else {
            Answer::Send(ok(b"the file"))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let done = get(&transport(Proxy::direct()), &server.url("/start"), &tmp.path().join("ok"), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"the file");
    let heads = server.heads();
    assert_eq!(heads.len(), 2);
    assert!(heads[1].starts_with("GET /dir/real?sig=1 HTTP/1.1\r\n"), "{}", heads[1]);

    // https → http is refused, however it is written, and nothing is requested from it
    for location in ["http://127.0.0.1:{port}/x", "HTTP://127.0.0.1:{port}/x", "ftp://127.0.0.1/x"] {
        let to_http = Server::start(LOCAL, LOCAL_KEY, move |_, _| {
            Answer::Send(format!("HTTP/1.1 301 Moved\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n").into_bytes())
        });
        let dir = tmp.path().join("http");
        // with every try allowed: a refused redirect is final, never asked again
        let r = get(&transport(Proxy::direct()), &to_http.url("/x"), &dir, &Options { attempts: 3, ..once() });
        assert!(matches!(&r, Err(FetchError::Transport(TransportError::Refused(m))) if m.contains("refused") || m.contains("unsupported")), "{r:?}");
        assert_eq!(to_http.heads().len(), 1);
        assert!(!dir.join("file.bin").exists());
    }

    // a redirect loop stops after the cap
    let looping =
        Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(b"HTTP/1.1 307 Again\r\nLocation: /again\r\nContent-Length: 0\r\n\r\n".to_vec()));
    let r = get(&transport(Proxy::direct()), &looping.url("/again"), &tmp.path().join("loop"), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Protocol(m))) if m.contains("too many")), "{r:?}");
    assert_eq!(looping.heads().len(), dalica_fetch::http::MAX_REDIRECTS + 1);
}

#[test]
fn a_stalled_server_times_out_and_cancel_stops_the_wait() {
    // silent after part of the body
    let server = Server::start(LOCAL, LOCAL_KEY, |i, _| {
        if i == 0 { Answer::Stall(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc".to_vec()) } else { Answer::Stall(Vec::new()) }
    });
    let tmp = tempfile::tempdir().unwrap();
    let opts = Options { stall_timeout: Duration::from_millis(600), ..once() };
    let t = Instant::now();
    let r = get(&transport(Proxy::direct()), &server.url("/f"), tmp.path(), &opts);
    assert!(matches!(r, Err(FetchError::Transport(TransportError::Stalled))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // silent before the head
    let t = Instant::now();
    let r = get(&transport(Proxy::direct()), &server.url("/g"), tmp.path(), &opts);
    assert!(matches!(r, Err(FetchError::Transport(TransportError::Stalled))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // a long stall timeout is cut short by Cancel
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::Relaxed);
    });
    let spec = FileSpec::new(&server.url("/h"), "h.bin", 1 << 20, None, None).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    let t = Instant::now();
    let opts = Options { stall_timeout: Duration::from_secs(60), ..once() };
    let r = download(&spec, &consent, tmp.path(), &transport(Proxy::direct()), &opts, &cancel, &mut |_| {});
    assert!(matches!(r, Err(FetchError::Cancelled)), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn oversized_headers_are_refused() {
    let server = Server::start(LOCAL, LOCAL_KEY, |i, _| {
        let mut r = b"HTTP/1.1 200 OK\r\n".to_vec();
        if i == 0 {
            // one header line past the cap
            r.extend_from_slice(format!("X-Big: {}\r\n", "a".repeat(dalica_fetch::http::MAX_HEAD)).as_bytes());
        } else {
            // many small ones
            for n in 0..=dalica_fetch::http::MAX_HEADERS {
                r.extend_from_slice(format!("X-{n}: v\r\n").as_bytes());
            }
        }
        r.extend_from_slice(b"Content-Length: 1\r\n\r\nx");
        Answer::Send(r)
    });
    let tmp = tempfile::tempdir().unwrap();
    for path in ["/long", "/many"] {
        let r = get(&transport(Proxy::direct()), &server.url(path), tmp.path(), &once());
        assert!(matches!(&r, Err(FetchError::Transport(TransportError::Protocol(m))) if m.contains("too")), "{path}: {r:?}");
    }
    assert!(!tmp.path().join("file.bin").exists());
}

#[test]
fn a_proxy_is_used_through_connect_and_skipped_for_no_proxy_hosts() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(ok(b"through")));
    let proxy = ProxyServer::start(200);
    let tmp = tempfile::tempdir().unwrap();
    let env = |vars: Vec<(&'static str, String)>| move |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());
    let via = Proxy::from_vars(env(vec![("HTTPS_PROXY", format!("http://127.0.0.1:{}", proxy.port))])).unwrap();
    let done = get(&transport(via), &server.url("/a"), &tmp.path().join("a"), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"through");
    let asked = proxy.heads();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with(&format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n", server.port)), "{}", asked[0]);
    // the proxy saw only the tunnel request: no path, no User-Agent, no Range
    assert!(!asked[0].contains("/a") && !asked[0].contains("User-Agent"), "{}", asked[0]);
    assert_eq!(server.heads().len(), 1);

    // NO_PROXY names the host: straight to the server
    let bypass =
        Proxy::from_vars(env(vec![("HTTPS_PROXY", format!("127.0.0.1:{}", proxy.port)), ("NO_PROXY", "example.org, 127.0.0.1".into())])).unwrap();
    get(&transport(bypass), &server.url("/b"), &tmp.path().join("b"), &once()).unwrap();
    assert_eq!(proxy.heads().len(), 1);
    assert_eq!(server.heads().len(), 2);

    // the manual setting
    let manual = Proxy::manual("127.0.0.1", proxy.port).unwrap();
    get(&transport(manual), &server.url("/c"), &tmp.path().join("c"), &once()).unwrap();
    assert_eq!(proxy.heads().len(), 2);

    // a proxy asking for a password fails, saying so
    let locked = ProxyServer::start(407);
    let r = get(&transport(Proxy::manual("127.0.0.1", locked.port).unwrap()), &server.url("/d"), &tmp.path().join("d"), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Connect(m))) if m.contains("password")), "{r:?}");
    assert_eq!(server.heads().len(), 3);
}

#[test]
fn a_certificate_that_does_not_validate_fails() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(ok(b"secret")));
    let tmp = tempfile::tempdir().unwrap();
    // the bundled web roots do not know the test root
    let bundled = HttpsTransport::new(Proxy::direct()).unwrap();
    let r = get(&bundled, &server.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Tls(m))) if m.contains("UnknownIssuer")), "{r:?}");
    // a certificate for another name, from a trusted root
    let other = Server::start(OTHER, OTHER_KEY, |_, _| Answer::Send(ok(b"secret")));
    let r = get(&transport(Proxy::direct()), &other.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Tls(m))) if m.contains("not valid for name")), "{r:?}");
    // no request reached either server, and nothing was written
    assert!(server.heads().is_empty() && other.heads().is_empty());
    assert!(!tmp.path().join("file.bin").exists());
    // the same server, trusted, by name too
    let done = get(&transport(Proxy::direct()), &format!("https://localhost:{}/f", server.port), tmp.path(), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"secret");
}

/// A server key that signs with one scheme only, so a handshake proves that scheme works.
#[derive(Debug)]
struct Only(Arc<dyn SigningKey>, SignatureScheme);

impl SigningKey for Only {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn rustls::sign::Signer>> {
        if offered.contains(&self.1) { self.0.choose_scheme(&[self.1]) } else { None }
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        self.0.algorithm()
    }
}

/// A server allowing only `version`, `suite`, `group` and `scheme`, with `cert` and `key`.
fn strict_server(
    version: &'static SupportedProtocolVersion,
    suite: CipherSuite,
    group: NamedGroup,
    scheme: SignatureScheme,
    (cert, key): (&str, &str),
) -> Server {
    let mut p = provider();
    p.cipher_suites.retain(|s| s.suite() == suite);
    p.kx_groups.retain(|g| g.name() == group);
    assert_eq!((p.cipher_suites.len(), p.kx_groups.len()), (1, 1), "{suite:?} {group:?}");
    let chain = vec![CertificateDer::from_pem_slice(cert.as_bytes()).unwrap()];
    let signer = p.key_provider.load_private_key(PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap()).unwrap();
    let certified = CertifiedKey::new(chain, Arc::new(Only(signer, scheme)));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(p))
        .with_protocol_versions(&[version])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(SingleCertAndKey::from(certified)));
    Server::with_config(Arc::new(config), |_, _| Answer::Send(ok(b"shaken")))
}

#[test]
fn every_suite_group_and_signature_scheme_handshakes() {
    use rustls::version::{TLS12, TLS13};
    let p256 = (LOCAL, LOCAL_KEY);
    let rsa = (RSA_LOCAL, RSA_KEY);
    let ecdsa = SignatureScheme::ECDSA_NISTP256_SHA256;
    let pss = SignatureScheme::RSA_PSS_SHA256;
    let x25519 = NamedGroup::X25519;
    let mut cases = Vec::new();
    // every suite: TLS 1.2's ECDSA ones with the P-256 key, its RSA ones with the RSA key
    for s in ALL_CIPHER_SUITES {
        let name = format!("{:?}", s.suite());
        let (version, scheme, key) = match (s.tls13().is_some(), name.contains("_RSA_")) {
            (true, _) => (&TLS13, ecdsa, p256),
            (false, true) => (&TLS12, pss, rsa),
            (false, false) => (&TLS12, ecdsa, p256),
        };
        cases.push((version, s.suite(), x25519, scheme, key));
    }
    // every group, in each version
    for group in [NamedGroup::X25519, NamedGroup::secp256r1, NamedGroup::secp384r1] {
        cases.push((&TLS13, CipherSuite::TLS13_AES_128_GCM_SHA256, group, ecdsa, p256));
        cases.push((&TLS12, CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256, group, ecdsa, p256));
    }
    // every signature scheme (TLS 1.3 has no PKCS#1 v1.5 handshake signatures)
    let schemes = [
        (SignatureScheme::ECDSA_NISTP256_SHA256, p256),
        (SignatureScheme::ECDSA_NISTP384_SHA384, (P384_LOCAL, P384_KEY)),
        (SignatureScheme::ED25519, (ED25519_LOCAL, ED25519_KEY)),
        (SignatureScheme::RSA_PSS_SHA256, rsa),
        (SignatureScheme::RSA_PSS_SHA384, rsa),
        (SignatureScheme::RSA_PSS_SHA512, rsa),
        (SignatureScheme::RSA_PKCS1_SHA256, rsa),
        (SignatureScheme::RSA_PKCS1_SHA384, rsa),
        (SignatureScheme::RSA_PKCS1_SHA512, rsa),
    ];
    for (scheme, key) in schemes {
        let rsa_key = key == rsa;
        let tls12 = if rsa_key { CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 } else { CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 };
        cases.push((&TLS12, tls12, x25519, scheme, key));
        if !format!("{scheme:?}").contains("PKCS1") {
            cases.push((&TLS13, CipherSuite::TLS13_AES_128_GCM_SHA256, x25519, scheme, key));
        }
    }
    assert_eq!(cases.len(), 9 + 6 + 9 + 6);

    let roots: Vec<Vec<u8>> =
        [ROOT, RSA_ROOT, P384_ROOT, ED25519_ROOT].iter().map(|r| CertificateDer::from_pem_slice(r.as_bytes()).unwrap().to_vec()).collect();
    let client = HttpsTransport::with_roots(Proxy::direct(), &roots).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    for (i, (version, suite, group, scheme, key)) in cases.into_iter().enumerate() {
        let server = strict_server(version, suite, group, scheme, key);
        let done = get(&client, &server.url("/f"), &tmp.path().join(i.to_string()), &once());
        let done = done.unwrap_or_else(|e| panic!("{:?} {suite:?} {group:?} {scheme:?}: {e}", version.version));
        assert_eq!(std::fs::read(&done.path).unwrap(), b"shaken");
        assert_eq!(server.heads().len(), 1);
    }

    // a chain from a root the client does not trust is refused, with any key type
    let only_p256 = transport(Proxy::direct());
    for (key, suite) in [(rsa, CipherSuite::TLS13_AES_128_GCM_SHA256), ((ED25519_LOCAL, ED25519_KEY), CipherSuite::TLS13_AES_256_GCM_SHA384)] {
        let scheme = if key == rsa { pss } else { SignatureScheme::ED25519 };
        let server = strict_server(&TLS13, suite, x25519, scheme, key);
        let r = get(&only_p256, &server.url("/f"), &tmp.path().join("untrusted"), &once());
        assert!(matches!(&r, Err(FetchError::Transport(TransportError::Tls(m))) if m.contains("UnknownIssuer")), "{r:?}");
        assert!(server.heads().is_empty());
    }
}
