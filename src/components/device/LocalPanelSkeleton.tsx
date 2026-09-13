import React from 'react'
import { Skeleton } from '@/components/ui/skeleton'

const LocalPanelSkeleton: React.FC = () => (
  <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-8 py-8">
    <div className="flex items-center gap-4">
      <Skeleton className="size-12 rounded-xl" />
      <div className="flex flex-1 flex-col gap-2">
        <Skeleton className="h-5 w-44" />
        <Skeleton className="h-3 w-28" />
      </div>
    </div>
    <Skeleton className="h-16 w-full" />
  </div>
)

export default LocalPanelSkeleton
