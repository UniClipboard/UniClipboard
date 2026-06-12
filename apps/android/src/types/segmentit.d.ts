declare module 'segmentit' {
  interface SegmentToken {
    w: string;
    p: number;
  }

  class Segment {
    doSegment(text: string, options?: { simple?: boolean }): SegmentToken[];
  }

  function useDefault(segment: Segment): Segment;

  export { Segment, useDefault, SegmentToken };
}
