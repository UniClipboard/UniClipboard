interface StepDotIndicatorProps {
  totalSteps: number
  currentStep: number
}

export default function StepDotIndicator({ totalSteps, currentStep }: StepDotIndicatorProps) {
  return (
    <div className="flex items-center justify-center gap-2">
      {Array.from({ length: totalSteps }, (_, i) => (
        <div
          key={i}
          data-testid={`dot-${i}`}
          className={`h-2 w-2 rounded-full ${
            i === currentStep ? 'bg-primary' : 'border border-muted-foreground/40'
          }`}
        />
      ))}
    </div>
  )
}
