# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems. Report them privately:

1. **Preferred:** GitHub private vulnerability reporting: on the repository page, open the **Security** tab and
   choose **Report a vulnerability**.
2. **Fallback**, if that is not available to you: email cott0wn98@gmail.com.

Include the version, steps to reproduce and the impact you see. You should get a first reply within
a few days; there is no bug bounty.

## Supported versions

Only the latest release receives security fixes.

## Known limitations

- Credentials (HTTP Basic logins, proxy credentials, URLs containing `user:password@`, `--cookie`/`-H` values)
  are stored **unencrypted** in the queue database (`~/.local/share/cawget/cawget.db`). Cawget creates the
  directory `0700` and the database `0600`; files that already exist keep their permissions. Do not share it.
- Cawget speaks HTTP and HTTPS only. TLS is handled by rustls; `cacert` can point at a custom CA file.
