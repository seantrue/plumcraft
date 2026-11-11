
The crates and the Python text twin are 0.6.1; the Python SDK stays 0.1.2. A patch: no public API changed. With installers: notarized DMGs for Apple silicon and Intel Macs, a Linux `.deb` and tarball, and a Windows setup program and zip. They are the first since 0.5.0, so they also bring 0.6.0's command forms and array fixes to the app.

- **The wire menu labels every use of an output** (#66): Draw as Label… is gone from a wire's right-click menu, leaving Label All Uses…, so turning a wire into a net label labels each wire from that output, as it used to.

