// The From field (§9.1 U6): the addresses the user can send as. A reply or forward lists only the thread mailbox's
// addresses (the server filters them); an address that cannot send yet says why.
import { ChevronDown, Danger } from 'reicon-react'
import type { MailFromAddress } from '@orbit/api/generated/types.gen'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { notReadyText } from './composeLib'

const label = (address: MailFromAddress) => (address.display_name.trim() ? `${address.display_name} <${address.address}>` : address.address)

export function FromPicker({ addresses, value, onChange }: { addresses: readonly MailFromAddress[]; value: string; onChange: (addressId: string) => void }) {
  const current = addresses.find((address) => address.address_id === value)
  const notReady = current && current.ready !== 'ready'
  return (
    <div className="flex min-h-9 items-center gap-2 border-b border-border/60 px-3">
      <span className="w-8 shrink-0 text-[0.8125rem] text-muted-foreground">From</span>
      {addresses.length > 1 ? (
        <DropdownMenu>
          <DropdownMenuTrigger
            className="-mx-1.5 flex min-w-0 items-center gap-1 rounded-md px-1.5 py-1 text-[0.8125rem] text-foreground outline-none transition-colors duration-100 hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 aria-expanded:bg-muted"
            aria-label="From"
          >
            <span className="truncate">{current ? label(current) : 'Choose an address'}</span>
            <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-auto max-w-[min(28rem,calc(100vw-2rem))] min-w-64">
            <DropdownMenuRadioGroup value={value} onValueChange={(next) => onChange(String(next))}>
              {addresses.map((address) => (
                <DropdownMenuRadioItem key={address.address_id} value={address.address_id} className="items-start py-1.5">
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate text-[0.8125rem]">{address.display_name || address.address}</span>
                    <span className="truncate text-xs text-muted-foreground">{address.address}</span>
                    {address.ready !== 'ready' ? (
                      <span className="mt-0.5 flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
                        <Danger className="size-3" />
                        {notReadyText(address.not_ready_reason)}
                      </span>
                    ) : null}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : (
        <span className="truncate text-[0.8125rem] text-foreground">{current ? label(current) : '—'}</span>
      )}
      {notReady ? (
        <span className="ml-auto flex shrink-0 items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
          <Danger className="size-3.5" />
          Cannot send yet
        </span>
      ) : null}
    </div>
  )
}
