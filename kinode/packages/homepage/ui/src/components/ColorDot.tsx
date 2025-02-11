import classNames from 'classnames'
import React from 'react'
import { hexToRgb, hslToRgb, rgbToHex, rgbToHsl } from '../utils/colors'
import { isMobileCheck } from '../utilities/dimensions'

interface ColorDotProps extends React.HTMLAttributes<HTMLSpanElement> {
  num: string,
}

const ColorDot: React.FC<ColorDotProps> = ({
  num,
  ...props
}) => {
  const isMobile = isMobileCheck()

  num = (num || '').replace(/(0x|\.)/g, '')

  while (num.length < 6) {
    num = '0' + num
  }

  const leftHsl = rgbToHsl(hexToRgb(num.slice(0, 6)))
  const rightHsl = rgbToHsl(hexToRgb(num.length > 6 ? num.slice(num.length - 6) : num))
  leftHsl.s = rightHsl.s = 1
  const leftColor = rgbToHex(hslToRgb(leftHsl))
  const rightColor = rgbToHex(hslToRgb(rightHsl))

  const angle = (parseInt(num, 16) % 360) || -45

  return (
    <div {...props} className={classNames('flex', props.className)}>
      <div
        className={classNames('m-0 align-self-center border rounded-full outline-black', {
          'h-16 w-16 border-4': !isMobile,
          'h-8 w-8 border-2': isMobile
        })}
        style={{
          borderTopColor: leftColor,
          borderRightColor: rightColor,
          borderBottomColor: rightColor,
          borderLeftColor: leftColor,
          background: `linear-gradient(${angle}deg, ${leftColor} 0 50%, ${rightColor} 50% 100%)`,
          filter: 'saturate(0.25)',
          opacity: '0.75'
        }} />
      {props.children}
    </div>
  )
}

export default ColorDot
