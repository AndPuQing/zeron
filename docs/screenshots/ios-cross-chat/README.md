# iOS cross-chat attribution screenshots

Unedited XCTest attachments from an **iPhone 17 Pro simulator**, iOS 26.5,
Xcode 26.6, 1206 × 2622. These are simulator captures, not physical-device
screenshots.

Source revision: `d0aeb259b55302bb93b06d3e60059eafe36b7a90`.
The macOS workflow run [39761214977](https://github.com/AndPuQing/zeron/actions/runs/39761214977),
job `ios-tests` (`119770244096`), executed 28 unit tests and 10 selected UI tests
with no failures. The target test was
`SessionFlowTests.testCrossChatMessageShowsSenderAndBody`.

Original files from the `ios-screenshots` artifact (`12420775388`):

- `collapsed.png`: `E5B21644-9682-4928-9E4C-90E26D6DA3C2.png`, attachment
  `cross-chat-collapsed`. The sender name is semibold and the beginning of the
  body is visible above `Show more`. The routing instruction and full routing
  UUID are absent.
- `expanded.png`: `188F65C5-EC93-4293-B5FE-1EFFA81C0A81.png`, attachment
  `cross-chat-expanded`. The body reaches its final sentence and the control
  reads `Show less`. At this scroll position the sender line is behind the
  translucent navigation header; use the collapsed capture to inspect it.

The offline demo uses fictitious titles and ids. Neither image was cropped,
stitched, or retouched.
