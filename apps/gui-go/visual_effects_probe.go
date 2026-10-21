package main

type evidenceClass string

const (
	evidenceUnknown     evidenceClass = "unknown"
	evidenceConstrained evidenceClass = "constrained"
	evidenceCapable     evidenceClass = "capable"
)

func classifyVisualCapabilities(cores uint64, memory uint64, graphics evidenceClass) evidenceClass {
	if (cores > 0 && cores < 4) || (memory > 0 && memory < 8*1024*1024*1024) || graphics == evidenceConstrained {
		return evidenceConstrained
	}
	if cores == 0 || memory == 0 || graphics != evidenceCapable {
		return evidenceUnknown
	}
	return evidenceCapable
}
