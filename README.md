# corr

## TLS Setup

Generate cert:

```
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -sha256 -days 3650 -nodes -subj "/C=XX/ST=StateName/L=CityName/O=CompanyName/OU=CompanySectionName/CN=CommonNameOrHostname"
```

Config:

```toml
[database]
uri = "sqlite:tempdb.db?mode=rwc"

[server]
address = "http://127.0.0.1:8080"
bind = "127.0.0.1:8443"
ssl_cert = "/Users/herman/Code/park/cert.pem"
ssl_key = "/Users/herman/Code/park/key.pem"
```
