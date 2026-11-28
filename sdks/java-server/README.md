# Skotos Java Server SDK (OpenJDK 21)

Pure Java 21 server-side SDK for Skotos with zero third-party JAR dependencies.

- 124/124 canonical golden evaluation vector parity with `crates/skotos-eval`
- Big Segment 64-bit FNV-1a unsigned binary search lookup
- `SkotosClient` using `java.net.http.HttpClient` with `ETag` / `If-None-Match` caching
- OpenFeature-compatible `SkotosOpenFeatureProvider`

## Running tests

```bash
javac -d sdks/java-server/build \
  sdks/java-server/src/main/java/dev/skotos/sdk/*.java \
  sdks/java-server/src/test/java/dev/skotos/sdk/GoldenSuiteTest.java
java -cp sdks/java-server/build dev.skotos.sdk.GoldenSuiteTest
```
