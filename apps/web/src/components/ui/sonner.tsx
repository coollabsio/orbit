"use client"

import { Toaster as Sonner, type ToasterProps } from "sonner"
// Sonner injects this CSS as a runtime <style> tag, which the production CSP (`style-src 'self'`) blocks (vite.config.ts turns that off).
// Importing the file bundles it into our stylesheet so toasts are styled in production too.
import "sonner/dist/styles.css"
import { TickCircle as CircleCheckIcon, InfoCircle as InfoIcon, Danger as TriangleAlertIcon, Danger as OctagonXIcon } from 'reicon-react'
import { Spinner } from "@/components/ui/spinner"
import { useTheme } from "@/lib/themeContext"

const Toaster = ({ ...props }: ToasterProps) => {
  const { theme } = useTheme()

  return (
    <Sonner
      theme={theme as ToasterProps["theme"]}
      position="bottom-right"
      className="toaster group"
      icons={{
        success: (
          <CircleCheckIcon className="size-4" />
        ),
        info: (
          <InfoIcon className="size-4" />
        ),
        warning: (
          <TriangleAlertIcon className="size-4" />
        ),
        error: (
          <OctagonXIcon className="size-4" />
        ),
        loading: (
          <Spinner aria-hidden />
        ),
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
