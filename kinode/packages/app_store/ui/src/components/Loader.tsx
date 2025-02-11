import React from 'react'

type LoaderProps = {
  msg: string
}

export default function Loader({ msg }: LoaderProps) {
  return (
    <div id="loading" className="flex flex-col text-center">
      <h4>{msg}</h4>
      <div id="loader"> <div /> <div /> <div /> <div /> </div>
    </div>
  )
}
