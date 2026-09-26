/*!
 * Stemma's animated mark (WebGL), embeddable and transparent.
 *
 *   const mark = StemmaLogo.mount(canvas, { mode: 2, intensity: 1, speed: 1, onBreath: v => el.style.setProperty("--glow", v) });
 *   mark.pause(); mark.play(); mark.set({ intensity: .8 }); mark.destroy();
 *
 * The canvas covers a 566×474 frame: the symbol, with room for the wordmark
 * below it at 73.2% of the height. Size it with aspect-ratio: 566 / 474.
 * Output is premultiplied alpha, so it composites over any background.
 * Modes: 0 pulse, 1 sheen, 2 pulse and sheen. It respects
 * prefers-reduced-motion (a single still frame) and pauses while the page
 * is hidden. Only the front page draws it, and only once.
 */
(function (global) {
  "use strict";
  var D = {"faces":[{"d":"M329.139 129.179C328.917 130.002 328.561 130.784 328.317 131.601C328.266 131.771 328.293 132.021 328.282 132.198C328.25 132.728 328.216 133.258 328.179 133.788C328.044 135.714 327.875 137.637 327.688 139.558C326.897 147.708 325.819 155.828 325.533 164.018C325.759 164.415 325.982 164.812 326.204 165.21C326.124 170.824 326.472 176.42 327.105 181.998C327.944 189.392 329.283 196.798 331.892 203.791C338.542 221.614 353.572 235.339 357.197 254.472C357.905 258.212 358.182 262.076 357.71 265.864C357.081 270.903 355.051 275.57 351.069 278.846C349.436 280.19 347.587 281.267 345.642 282.092C340.228 284.388 334.691 284.436 329.058 282.966C323.338 281.475 317.993 278.751 312.716 276.149C305.918 272.798 299.199 269.282 292.313 266.111C287.671 263.973 282.953 262.009 278.123 260.336C270.403 257.661 262.515 255.511 254.819 252.762C249.383 250.821 244.015 248.697 238.663 246.539C230.088 243.082 221.435 239.559 212.294 237.907C209.846 237.464 207.384 237.184 204.902 237.028C204.925 236.833 205.109 236.636 205.113 236.44C205.113 236.384 204.976 236.388 204.982 236.331C204.774 236.607 204.408 236.725 204.176 236.981C204.093 237.073 203.893 237.794 203.838 237.958C202.978 240.482 202.225 243.049 201.728 245.671C201.046 249.262 200.859 252.985 201.639 256.577C203.498 265.133 210.477 272.652 217.943 276.854C220.909 278.523 224.105 279.764 227.405 280.587C237.642 283.141 248.408 281.607 258.815 281.399C263.237 281.311 267.66 281.364 272.082 281.344C277.578 281.319 283.075 281.206 288.563 280.894C289.21 280.858 289.885 280.867 290.526 280.774C290.969 280.71 291.414 280.561 291.851 280.463C292.677 280.279 293.506 280.11 294.338 279.958C296.426 279.578 298.536 279.308 300.654 279.169C315.415 278.199 327.185 283.709 337.924 293.473C339.578 294.977 341.177 296.54 342.764 298.116C343.303 298.651 343.841 299.188 344.381 299.724C344.623 299.964 344.856 300.245 345.122 300.457C345.714 300.93 346.394 301.322 347.018 301.753C348.261 302.611 349.498 303.479 350.739 304.34C355.318 307.519 359.989 310.65 365.139 312.836C379.441 318.907 395.957 317.473 408.608 308.296C414.049 304.349 418.456 299.114 421.508 293.13C424.128 287.994 425.764 282.341 426.181 276.586C427.221 262.223 420.822 247.909 410.648 237.952C407.962 235.323 404.996 232.994 401.804 231.009C398.264 228.807 394.509 226.981 390.88 224.936C386.741 222.604 382.74 220.034 378.83 217.339C372.375 212.892 366.165 208.048 360.59 202.526C348.909 190.953 340.315 176.406 335.293 160.778C333.941 156.57 332.914 152.286 331.975 147.971C331.497 145.772 331.022 143.572 330.478 141.389C329.955 139.289 329.314 137.202 329.084 135.043C328.877 133.095 329.044 131.128 329.139 129.179Z","x0":186.0,"y0":114.0,"h":6.0,"s":5.0,"nx":43,"ny":36,"cells":"sMr5/6rG+P+lwvf/n772/5q59f+UtfP/ja/w/4aq7f9/o+j/dpzj/26T3P9ki9T/W4HM/1F4w/9Gbrn/PGSv/zJbpv8oU53/H0uW/xdFj/8QQIr/Cj2I/wc8h/8GPon/CEOO/w1Klv8UU5//HF6r/ydruf8yeMf/P4fX/0yX6P9ap/r/abj//3jK//+G2///le3//6T///+z////wf///8/////d////7P///6O/8f+fvPD/m7nw/5e27/+Ssu7/ja/t/4ir6/+Dpun/fKHl/3ab4f9vldz/Z43W/1+Gz/9Wfcj/TXXA/0Rtt/87ZK//M12n/ypWoP8jUJr/HEuV/xdIkv8TR5L/EkiU/xRMmP8YU6D/H1yq/ydntf8xc8L/PIDQ/0iO3/9Vne//Yq3//3C9//99zv//i97//5nu//+m////tP///8H////P////3P///+n///+XtOj/lLLo/5Gw6P+Oruj/iqvo/4ep5/+Dpub/f6Pl/3qf4/91m+D/cJbc/2mQ2P9jitP/W4PN/1R8xv9Mdb//RG24/z1msf82YKr/L1ql/ylWoP8kU53/IFGc/x5Snv8fVaL/JFyq/ypltP8yb7//O3rM/0WH2f9Rlef/XaT2/2qz//92wv//g9H//5Dg//+c8P//qf///7X////C////zv///9r////m////i6ng/4mo4f+Hp+H/habh/4Ok4f+Bo+H/fqHh/3uf4f94neD/dZre/3CW3P9sk9r/Z47W/2GJ0v9bg83/VH3H/012wf9HcLv/QGq1/ztlsP82Yav/MV6p/y1cp/8rXKj/LF+t/zBltf81bb7/PHfJ/0WC1f9PjuL/Wpzv/2aq/f9xuP//fcf//4nV//+U4///oPH//6v+//+2////wv///83////Y////5P///3+f2f9+ntn/fZ7a/3ye2v97ndv/ep3b/3mc3P93m9z/dprc/3SZ3P9xl9z/bpXb/2qS2f9mjtb/YYrT/1yFz/9WgMr/UHrF/0t0v/9Fb7r/QWu2/z9ptP88aLP/Omi1/zpquf88b8D/QHXI/0Z+0v9Oid7/WJbq/2Oj+P9usf//eb7//4TL//+O2P//meX//6Px//+t/v//uP///8L////M////1////+H///90ldL/dJXS/3SV0/90ltT/dJbV/3SX1f90l9f/c5fY/3OX2f9yl9r/cZfb/3CW2/9uldv/a5Pa/2iQ2f9kjdb/X4nT/1qEzv9Vfsr/UHnF/0x1wf9Kc7//SnPA/0t1w/9Jdsb/RnfJ/0h70P9PhNr/WJDm/2Kd8/9sqv//drf//4DD//+Kz///k9v//5zm//+m8v//r/7//7n////C////zP///9X////f////aIvL/2mMzP9qjcz/a47N/2yPzv9tkM//bpHR/2+T0/9wlNX/cZXX/3GW2f9xl9v/cZjc/3CX3f9ult7/a5Td/2iR2/9kjdj/X4nU/1mDz/9Ufsv/UXrI/1F4yP9QeMn/S3bK/0l2zf9Of9X/WIvi/2KY8P9spf3/dbH//369//+HyP//j9L//5fd//+g5///qPL//7H9//+5////wv///8v////U////3f///16Cxf9ghMb/YYXG/2OGx/9liMj/ZorJ/2iMy/9qjs3/bJDQ/26T0/9wldb/cZfZ/3OZ3P9zmt//c5vh/3Kb4/9wmuP/bZfi/2iT3/9jjdr/XIbV/1R+zv9KdMj/QGrB/ztmwP9Fccn/VILZ/2GS6v9soPn/daz//323//+Fwv//jMv//5PV//+b3v//ouj//6ry//+y+///uv///8L////K////0////9v///9UesH/VnzB/1l+wf9bf8H/XYHC/2CDw/9ihsX/ZYjH/2iMyv9rj87/bpLS/3GW1v9zmdr/dpzf/3ef4/94oOb/d6Hp/3Wg6v9ynen/bJfl/2OP3v9Wg9T/R3XI/zpovP88abv/TXjL/16K4P9qmvP/c6f//3uy//+DvP//icX//5DN//+X1v//nd7//6Tn//+r8f//s/r//7r////C////yv///9L////Z////S3O9/051vP9Qd7z/U3i8/1Z6vP9Zfb3/XH++/2CDwf9jhsT/Z4rI/2uPzP9vk9H/c5jX/3ec3f96oOL/fKTo/32m7f98p/D/eqXx/3Wh7/9rmen/Xo3e/059zf9Dcb7/SHG8/1Z+z/9jj+f/bZ35/3eq//9/tP//hr3//4zF//+Szf//mNX//5/e//+l5v//rO///7P4//+7////wv///8n////Q////2P///0Ntuv9Gb7n/SXC4/0xyt/9PdLf/Una3/1Z5uP9afLr/XoC9/2OFwf9oisX/bI/L/3GV0f92mtj/eqDf/36l5v+Bqe3/gqzz/4Gs9/99qvj/daT0/2qZ6v9bidn/SHPA/ztjsf9EbcH/WIbg/2ma9v92qP//gLP//4e8//+NxP//k8z//5nU//+f3P//puX//63t//+09v//uv7//8H////I////0P///9f///88aLf/P2m2/0JqtP9FbLP/SG6y/0twsv9Pc7L/VHaz/1h6tf9dfrj/Y4S9/2iJwv9ukMn/dJbQ/3md2f9+pOL/gqnr/4Wu8/+Fsfr/g7H9/3+t/f93pfb/aZbn/1J8yv82X6n/MVuo/0Zzx/9ejeX/caD5/3yt//+Etv//i7///5HI//+Y0f//n9r//6bi//+t6///s/P//7r7///B////yP///8/////W////NWO2/zhktP87ZbL/Pmew/0Jorv9Faq3/SWys/01vrP9Sc63/V3ev/1x8s/9igrj/aYm+/2+Qxv92mM//fKDZ/4Gn5P+Gru//iLP4/4i1//+Fs///fqz9/3Gd8P9Zg9P/O2Ss/y1Vm/86Yq7/UnzP/2eS5/90ovf/fa3//4W4//+Owv//ls3//57X//+l4P//rOj//7Lw//+5+P//wP///8f////O////1f///zBftP8zYLL/NmGw/zlirv88Y6v/P2Sp/0Jmp/9GaKX/S2ul/1Bvpv9Vc6j/W3mr/2J/sf9ph7n/cJDC/3eZzv9+otr/hKrn/4ix8/+Ktfz/h7X//3+u//9vnvH/VYLV/zljsf8wVZ7/N1ym/0lvvf9bhNT/Z5Pl/3Kh9P9+r///ibz//5PI//+c0///o9z//6rk//+x7P//uPT//7/8///G////zf///9T///8rW7L/Llyw/zFdr/80Xqz/Nl+p/zlfpv88YKL/QGGf/0RjnP9IZZv/TWmb/1Junf9ZdKH/YHyp/2iFs/9wj7//eJrN/4Ck2/+Fren/iLL1/4az/P9+rfv/bp7w/1aF2f9Aabv/N1yo/zlgqP9CbbX/TnjF/1iD1f9mk+b/d6b2/4W2//+Qw///mc7//6DX//+o4P//r+j//7bw//+++P//xf///8z////T////J1iw/ypZr/8tWq7/L1us/zJbqP80W6T/N1uf/zlbmv88W5T/P1uP/0Ndjf9IYYz/TmeQ/1Vulv9eeKD/Z4Ot/3CPvP95msz/gKXc/4Ss6f+Dr/L/fav1/3Gh8P9ikOL/U3zM/0drtv89Zqz/Pmuy/0Vvvv9Nd8v/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drX//3u7//9/vv//gb3//4G7//9+tf//dqz//2Od+f9IivT/SYz1/53F+//8/////f////H4///1+v//7fT9/+Lv/v/e7v//3u3//+Ds///g6/z/4+32/+jx8v/u9P//8vj///b8//8NQo//FUqa/x1Rpf8kWLD/K1+6/zFmxP83bM3/PXPX/0N54f9JgOv/T4f2/1WP//9dmP//ZKL//2yr//9ztP//eLz//33B//9/xP//gMP//32///94uf//b67//2Cg//9Nkf3/QYf2/1WT8/+ixPT/9vr6//z//f/t9///6/f//+74///s9v//5/T//+fz///q8fz/7PD2/+zx8v/t8/7/8Pb///P6///2/P//DUKO/xZMnf8fVav/KF65/zBnx/85cdX/QXri/0mD8P9QjP7/WJT//2Cd//9npv//bq///3W4//96v///fsX//4HJ//+By///f8r//3vF//91vv//bLT//2Co//9Tmv//SpD//1CR+P9wo/H/q8bt/+Po7v/x8/T/6PD4/+nz/f/y+P7/9fn9//P3+v/y9vj/9PX1/+/1/f/v9f//8Pf///L4///0+///9vz//w1Cjv8YTp//Ilmx/yxkwv82cNT/QHvm/0qH+P9Uk///Xp7//2ip//9xtP//eb7//3/G//+Fzv//iNP//4nW//+I1v//hNT//33O//90xf//abv//16v//9Uo///Tpr//1KX//9knvr/hK7w/6zD6f/N1ub/3+Lp/+fo7f/v7/L/9Pb6//T6///z+f//8/n///P5///z+f//8/n///P6///0+v//9Pr///X8//8OQo3/GU+h/yRdt/8wasz/PHjh/0iG9/9Ulf//YKP//2yx//93vv//gcv//4rV//+R3v//leT//5bn//+U5v//j+P//4bc//960v//bMX//123//9Qqv//SqD//0+d//9foP//d6r8/5K17/+ovuX/vMfg/8/T4P/i3+P/5uz3/+71///y+P//9Pr///X7///1+///9fv///X8///1+///9fv///X7///1+///"},{"d":"M208.644 225.275C208.888 225.031 209.296 225.019 209.567 224.805C209.604 224.776 209.872 223.997 209.912 223.891C221.214 218.424 232.054 211.658 240.94 202.706C254.253 189.296 262.494 171.314 277.721 159.722C281.862 156.57 286.467 154.017 291.449 152.47C296.523 150.894 301.959 150.355 307.201 151.363C313.753 152.623 319.788 156.209 323.848 161.528C324.737 162.693 325.493 163.93 326.204 165.21C326.411 156.707 327.539 148.281 328.366 139.826C328.632 137.102 328.867 134.374 329.016 131.641C329.145 129.259 329.269 126.896 328.959 124.522C328.327 119.679 326.287 114.503 324.211 110.097C321.325 103.975 317.612 98.861 312.341 94.573C301.74 85.949 287.144 82.222 273.698 84.615C264.503 86.252 256.031 90.89 249.216 97.21C245.525 100.634 242.296 104.559 239.642 108.837C236.925 113.217 234.823 117.969 233.463 122.943C231.284 130.917 230.892 139.233 230.441 147.444C229.933 156.696 229.4 166.029 227.154 175.054C226.014 179.637 224.444 184.109 222.513 188.417C221.614 190.424 220.574 192.373 219.641 194.351C219.487 194.676 219.385 195.038 219.261 195.376C219.006 196.064 218.754 196.752 218.502 197.441C217.699 199.639 216.911 201.842 216.136 204.049C213.652 211.132 211.28 218.247 208.644 225.275Z","x0":192.0,"y0":66.0,"h":6.0,"s":5.0,"nx":25,"ny":29,"cells":"6O75//H3///y+P//8/n///T6///0+///9fz///b8///3/f//9/7///j+///4/v//+P////j////4////+P////j////4/v//+P7///f+///3/f//9/3///X7///v9f//6vD7//H4///y+P//8/n///T6///0+///9fv///b8///2/f//9/3///f+///4/v//+P////j////5//7/+v/+//n//v/5////+P////j+///4/v//9/7///f9///0+v//8Pb//+zz/f/z+f//9Pr///T7///1+///9fz///b8///2/f//9/3///f+///4/v//+P////j////6//7/9f38/+/6+//7//3/+//9//n//v/4////+P7///j+///3/f//9Pr///H3///u9P//9fv///X8///2/P//9vz///b9///3/f//9/3///f+///4/v//+P7///T6+v/v9/r/5vH8/9Pl/f/K4v3/2uv+/+/3/v/6/fz/+v/+//j////3/f//9fv///T6///y+f//8ff///f9///3/f//9/3///f+///3/v//9/7///j+///1+///6vH0/9rp9f/M4fv/xd39/8Xd/f/B2v7/vdf+/8Td/v/S5f7/3+7//+z2/v/2+vr/9fv///X7///0+v//8/n///L4///8//3/+f/+//j////4////+P////j////z+f//4u3y/83g9P+91/n/stH+/6zN/v+vz/3/tdL9/7nU/f+71/3/wtr+/83h/v/c7P//6/X///X5+//3+/3/9fv///T6///z+f////////////////////////3//v/4/v7/4+/4/8re+P+yz/r/psn8/6TH/P+gxPr/n8L5/6XG+/+tzfz/s9L9/7nU/v/A2f3/zuL+/+Lv///0+v//+/38//b8///0+v//8vj/////////////////////////////5/b//83i/P+yz/v/mb/6/5G5+P+TufT/krj0/5O59v+XvPf/ncH5/6XH/P+vzv7/udb9/8fe/P/e6/7/9Pr///z+/f/4+vz/8/n///L4////////////////////////8f///9Hp//+00/z/mr/3/4iy9f+ErvL/hazv/4iu7/+KsvH/i7Ty/4629f+Xvfj/p8j8/7bT/f/F3P3/2uj+/+32///v+f3/7fX6//P09//x9////////////////////////+P9//+73P//mMD5/4Ku8f99qO7/f6fs/3yk6v96o+j/fKbp/4Cq7P+Gr/L/krn1/6LE+f+wzv3/wdn9/9nn/v/q9P//6vb//+v1/f/w9Pn/8fD0///////////////////////Y+P//qtD//4Wx9/9yoOv/cp3l/3Od4/9vmeL/a5Xg/26X4f93n+f/g6vu/5C38/+dwvj/qMv8/7vY/f/Z6f3/8vj+///////9//7/8/T4/9vk9P//////////////////////1Pj//6LJ//99qPH/bJfj/2mT3P9kjtj/YYvY/2KM2P9okNr/cpjg/3yj6P+IsfH/msH6/6/Q/v/G4P7/3+39//X6/v///////Pz8/9vn+P+0zvX//////////////////////9n///+exv//c5zo/2CK1v9dh9L/VoDO/1R+zP9ahc7/YozR/2yU2v93oOf/g672/5zD///D3f//5PH///H4///z+v//5fP//8Pd/P+ix/n/hbP2///////////////////////m////osv//2mS4f9OeMf/UHzG/055xP9MdsH/UHvF/1iCzP9ok9z/g6zx/5i//v+u0P//1Of9//L4/f/2+f7/6vT//8fh//+Ww/z/dq76/2Cf+P//////////////////////+v///7Lc//9qlOL/Q228/0hyvP9Lc7z/SG+5/0t0wP9Sfcz/ZZHi/5C3+P+61v//0OX7/+Hu9v/r8/b/5e/4/9jo+/+82P3/lMH9/3Cs+/9Rmfr////////////////////////////F8f//cZzn/ztktP89ZbH/Qmi0/0Fnsv9Jc7v/VYHO/2eU5P+Qt/f/wtv8/9/u+P/m8vT/4+7z/9jn9P/K3/b/t9T4/5vF+v95svv/VZ37////////////////////////////0////3aj7P8yXK3/MVik/zpgqv86Yq3/SXS+/2CL1v93oer/l7z3/77Z+//Z6/r/4fD3/9zs9f/R5fT/w9z0/7PT9v+dx/j/grj6/2Km/P///////////////////////////9X///93pu//L1iq/y1Smf85XqT/PGOv/094xv9tl+H/iLDy/6LF+/+82f//0Ob+/9js/P/W6vr/zeX4/8Dd9/+y1Pj/oMr5/4u//P90s/7////////////////////////////J9///c6Dt/y5XqP8sUZf/OWCl/0Botf9XgM//d6Dp/5K6+v+ozP//u9r//8jk///P6f//z+n//8nl/v/A3/3/tNj9/6bQ/f+XyP//h8D////////////////////////k////sN7//2WS4/8qUqL/KlCY/ztjrv9Jc8L/YYza/36o8f+Yv///q9D//7nb///D4///yef//8rp///H5///weP//7je//+u2f//pNP//5nO///////////////////d////t+T//5HA//9SftL/JUuX/y1TnP9Fa7r/WYLT/2+Z6f+Gr/r/m8P//6vS//+33P//wOP//8Xn///H6v//xur//8Po//++5v//uOP//7Lg//+r3f/////////////t////tOT//469//9wnvH/PGe3/yJHkP82XKj/U3rM/2mR5f98pff/jrf//57G//+r0///tt3//73k///C6f//xez//8fu///G7v//xe7//8Lu///A7f//vez/////////////2f///6LU//95qvb/VoTX/zBap/8uVp//SXHB/2OL4f92n/X/hq///5S9//+gyv//q9T//7Te//+85f//wuv//8bv///J8///y/X//8z3///N+f//zfr//878/////////////83///+bz///aZrr/z5rvv8xWqj/Qmy6/1uF2P9umO//fqj//4u1//+Xwf//osz//6vW//+03///vOf//8Lu///I8///zfj//9H9///U////2P///9v////e/////////+T///+57f//h7n//0p50f8lT6X/LFWm/0hxwv9fit7/cZzy/4Cq//+Mt///mMP//6LN//+s1///tOH//7zp///E8f//y/j//9H////X////3f///+P////o////7v///+P///+98P//lcf//2OU6P8uW7b/Fj+X/yNKn/8+Z7v/V4HX/2yW7f98p///irX//5bC//+hzv//rNn//7Xj//++7f//x/b//8/+///X////3v///+b////u////9v////3///+w5P//jb///2eY6v9Absb/HUmk/xA5k/8bQpv/MVmx/0pzy/9hi+P/dJ/4/4Ww//+TwP//oM3//6va//+25f//wPD//8r6///T////3f///+b////w////+f//////////////dqn//1iJ4v87asT/IU6p/xA6lv8LNJD/FDyX/yZOqP89Zb//VH7X/2qV7/99qv//j7z//57M//+r2///t+j//8P0///O////2P///+P////u////+f///////////////////zls0/8jU7j/ET6g/wUxkP8CLIn/BS6L/w42k/8dRKH/MFi0/0dwzP9fieX/daL9/4q4//+by///q9z//7nr///F+P//0f///93////p////9v////////////////////////8="},{"d":"M292.472 279.612C291.824 279.755 291.058 279.988 290.419 280.08C290.06 280.131 289.683 280.127 289.321 280.149C288.523 280.197 287.724 280.241 286.924 280.281C284.383 280.407 281.839 280.493 279.295 280.551C270.522 280.748 261.745 280.496 252.977 280.901C240.019 281.501 227.649 282.473 216.234 274.996C212.793 272.742 209.711 269.899 207.227 266.617C203.234 261.343 201.344 255.751 201.944 249.121C202.504 242.926 204.829 237.032 207.068 231.286C208.479 227.664 209.883 224.039 211.206 220.384C213.23 214.789 215.085 209.135 217.063 203.523C219.201 197.458 221.468 191.442 223.779 185.441C222.986 187.36 222.158 189.259 221.243 191.123C220.841 191.941 220.37 192.737 219.995 193.567C219.923 193.726 219.882 193.898 219.807 194.056C219.492 194.727 219.058 195.367 218.679 196.003C217.741 197.579 216.757 199.128 215.733 200.65C211.476 206.977 206.532 212.822 201.202 218.268C196.802 222.765 192.129 227.013 187.079 230.773C183.471 233.46 179.671 235.895 175.675 237.963C174.935 238.345 174.189 238.716 173.436 239.072C173.063 239.249 172.688 239.455 172.301 239.598C172.134 239.66 171.954 239.685 171.788 239.749C171.016 240.048 170.029 240.622 169.276 241.017C166.866 242.281 164.513 243.663 162.284 245.228C159.077 247.479 156.176 250.127 153.773 253.23C144.497 265.209 142.723 282.432 148.102 296.428C152.981 309.122 163.963 318.848 176.584 323.525C193.002 329.609 209.376 326.24 223.82 316.958C230.393 312.734 236.348 307.644 242.693 303.099C248.776 298.742 255.165 294.78 261.922 291.551C264.328 290.402 266.78 289.347 269.276 288.407C270.408 287.981 271.567 287.628 272.699 287.203C273.471 286.913 274.219 286.526 274.977 286.198C276.542 285.522 278.118 284.87 279.708 284.252C282.096 283.325 284.514 282.475 286.969 281.742C287.774 281.502 288.582 281.275 289.394 281.061C289.68 280.986 290.165 280.932 290.414 280.802C291.116 280.434 291.759 279.958 292.472 279.612Z","x0":132.0,"y0":168.0,"h":6.0,"s":5.0,"nx":29,"ny":29,"cells":"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PWzC/zVitf8xWqv/KlOi/yNNm/8hSpX/H0iP/x1Gj/8kTJr/Nl6w/1B60P9unPr/i7///6ff//+++v//0v///+T////1////////////////////l9T//4vH//+Auv//c6z7/2ac8f9YiuT/TnzY/0l1zv9Ab8b/Omi7/zhhsv8wW6r/KVWj/ydRnv8oUZ3/LFSk/zljuP9QfNf/bZv8/4u9//+p3///xf///97////1//////////////////////////////+U1v//isr//4C9//91sP3/aaL0/16T6v9ZieL/VYLc/0l40f9AbsL/PWe7/zpnu/83ZLj/N2C0/zpit/9Da8T/VH7d/2yZ/v+Iuf//ptv//8T9///h/////f///////////////////////////////////5DV//+IzP//gcH//3m3//9xrPv/a6P2/2md8/9ll+7/WYrj/09+1v9Kd9L/SnnV/0t61v9Nedb/U33b/1+I6/9xnP//iLf//6PW///B9///3/////3/////////////////////////////////////////i9T//4bN//+Cxv//fr///3u5//95tP//eLD//3Sq//9soPn/Y5by/16Q8P9ej/P/YJH3/2WU+/9umv//e6j//427//+k1f//v/P//9v////6//////////////////////////////////////////////+F0///hM///4TL//+EyP//hsf//4bF//+Gwv//grz//321//94r///dKr//3On//92qf//fq///4m4//+Yx///qtv//8Hz///a////9v///////////////////////////////////////////////////w=="},{"d":"M263.747 290.702C270.711 287.611 277.968 285.235 285.543 284.234C302.328 282.015 321.409 287.119 336.19 295.031C340.63 297.407 344.794 300.202 348.923 303.075C346.291 300.72 343.849 298.169 341.317 295.71C338.315 292.793 335.186 289.999 331.776 287.564C319.361 278.7 305.84 276.472 291.075 279.923C281.571 282.144 272.439 286.338 263.747 290.702Z","x0":246.0,"y0":264.0,"h":6.0,"s":5.0,"nx":20,"ny":9,"cells":"p6uz/73Cyv/T2OL/6O75//P5///v////7P///93////B/v//mNb//22s//9UkvH/S4bq/0V95v86ceH/KmDa/xdN0v8DOcj/ACO+/wAOtP9VV1v/bG90/4OGjP+bn6b/rLi8/6rV5v+z5v//rOT//5nQ//+Ct/z/XpTm/02D3f9Ogt7/SXze/z1x2v8vY9T/IlXO/xVGx/8IN8D/ACi5/wUFBf8bHB3/LDk5/z5TZf9Qb5P/ZY/F/3qt9v93rf7/aJno/2iU3/9Vg9H/SXfL/0160P9HeNT/PG/S/zRlzv8uXMn/KFTG/yJLwv8bQ7//AAAA/wAAAP8AAB3/AAZG/wYlcv8hSaP/PG3U/z1w3P8yYMX/P2nD/0Zvwv9Da7//RW/D/z5wyf87bMn/PGnH/zxmxv88Y8X/PGDE/zxdxP8AAAD/AAAA/wAAA/8AACv/AABV/wADgP8AJKb/AjGz/wUyrP8XQq7/K1S0/zZetv86Zrr/Omu//0FtwP9Ib8L/THHE/1Fzxv9Xdsf/XXnJ/wAAAP8AAAD/AAAA/wAAEf8AADf/AABb/wAAef8AAIv/AAmU/wAfnP8NOKX/Ik2s/zFdsv89abj/S3K8/1R4wP9efsP/aIXH/3OMy/9+lM//AAAA/wAAAP8AAAD/AAAA/wAAF/8AADb/AABQ/wAAZv8AAHj/AACH/wAclP8OOaD/J1Gp/z5lsv9RdLn/YYG+/3CMxP+AmMn/kKTP/6Cx1P8AAAD/AAAA/wAAAP8AAAD/AAAA/wAAEf8AACv/AABE/wAAW/8AAHD/AACD/wAkk/8cRKD/O1+r/1Z2tf9tib3/g5vF/5mszP+uvdP/w87a/wAAAP8AAAD/AAAA/wAAAP8AAAD/AAAA/wAAB/8AACL/AAA+/wAAWf8AAHH/AA+F/xE2l/83WaX/Wnex/3qSvP+Xqsb/ssDO/8zW1//k6vT/"}],"dot":[284.6016237170965,224.00466192804285,15.220848217430786],"dotColor":[0.4627450980392157,0.6745098039215687,0.9921568627450981],"bgc":[285.5,215.5],"bgR":330.0,"bgStops":[[0.02745,0.09412,0.24706],[0.02745,0.09412,0.24706],[0.02353,0.0902,0.23922],[0.01569,0.07451,0.20784],[0.01176,0.06275,0.18039],[0.01176,0.0549,0.16078]],"k":[0.02296991685572814,0.05951203280845843,0.05960844127487717],"sig":[4.0,12.0,32.0]};
  var REF_W = 566, REF_H = 474;

  var VS = "attribute vec2 pos; void main(){ gl_Position = vec4(pos, 0.0, 1.0); }";

  // Smooth colour field of one face: grid of colour cells convolved with a Gaussian, clipped by the face outline.
  var FS_FIELD = [
    "precision highp float;",
    "uniform vec2 uView; uniform sampler2D uCells; uniform vec2 uN; uniform vec2 uOrig; uniform float uH, uS; uniform sampler2D uMask;",
    "const vec2 REF = vec2(566.0, 474.0);",
    "float erf_(float x){ float s = sign(x); x = abs(x); float t = 1.0/(1.0 + 0.3275911*x);",
    "  float y = 1.0 - (((((1.061405429*t - 1.453152027)*t) + 1.421413741)*t - 0.284496736)*t + 0.254829592)*t*exp(-x*x); return s*y; }",
    "void main(){",
    "  vec2 uvm = vec2(gl_FragCoord.x/uView.x, 1.0 - gl_FragCoord.y/uView.y);",
    "  float m = texture2D(uMask, uvm).a; if (m <= 0.0) discard;",
    "  vec2 p = uvm*REF; float k = 1.0/(uS*1.41421356); vec2 ij = floor((p - uOrig)/uH);",
    "  vec3 acc = vec3(0.0); float wsum = 0.0;",
    "  for (int dj = -3; dj <= 3; dj++) { for (int di = -3; di <= 3; di++) {",
    "    vec2 c = ij + vec2(float(di), float(dj));",
    "    if (c.x < 0.0 || c.y < 0.0 || c.x >= uN.x || c.y >= uN.y) continue;",
    "    vec2 lo = uOrig + c*uH;",
    "    float bx = 0.5*(erf_((lo.x + uH - p.x)*k) - erf_((lo.x - p.x)*k));",
    "    float by = 0.5*(erf_((lo.y + uH - p.y)*k) - erf_((lo.y - p.y)*k));",
    "    float w = bx*by; acc += w*texture2D(uCells, (c + 0.5)/uN).rgb; wsum += w; } }",
    "  gl_FragColor = vec4(acc/max(wsum, 1e-6)*m, m);",
    "}"].join("\n");

  var FS_DOT = [
    "precision highp float;",
    "uniform vec2 uView; uniform vec3 uDot; uniform vec3 uDotCol; const vec2 REF = vec2(566.0, 474.0);",
    "void main(){ vec2 p = vec2(gl_FragCoord.x/uView.x, 1.0 - gl_FragCoord.y/uView.y)*REF; float px = REF.x/uView.x;",
    "  float a = clamp(0.5 - (length(p - uDot.xy) - uDot.z)/px, 0.0, 1.0); gl_FragColor = vec4(uDotCol*a, a); }"].join("\n");

  var FS_BLUR = [
    "precision highp float;",
    "uniform sampler2D uSrc; uniform vec2 uView; uniform vec2 uDir; uniform float uSigma;",
    "void main(){ vec2 uv = gl_FragCoord.xy/uView; vec4 acc = vec4(0.0); float ws = 0.0;",
    "  for (int i = -48; i <= 48; i++) { float x = float(i); if (abs(x) > 3.0*uSigma + 1.0) continue;",
    "    float w = exp(-0.5*x*x/(uSigma*uSigma)); acc += w*texture2D(uSrc, uv + uDir*x/uView); ws += w; }",
    "  gl_FragColor = acc/ws; }"].join("\n");

  // Final composite: mark + breathing halo + sheen, as premultiplied alpha (no background).
  var FS_MAIN = [
    "precision highp float;",
    "uniform vec2 uRes; uniform float uTime, uIntensity, uMode;",
    "uniform sampler2D uLogo, uB0, uB1, uB2; uniform vec3 uK; uniform vec3 uDot;",
    "const vec2 REF = vec2(566.0, 474.0); const vec2 C = vec2(286.0, 216.0);",
    "float luma(vec3 c){ return dot(c, vec3(0.2126, 0.7152, 0.0722)); }",
    "void main(){",
    "  vec2 uvT = gl_FragCoord.xy/uRes; vec2 p = vec2(uvT.x, 1.0 - uvT.y)*REF;",
    "  float t = uTime, I = uIntensity; int mode = int(uMode + 0.5);",
    "  bool pulseOn = (mode == 0 || mode == 2), sheenOn = (mode == 1 || mode == 2);",
    "  float breath = pulseOn ? 0.5 - 0.5*cos(t*1.7) : 0.3;",
    "  vec4 logo = texture2D(uLogo, uvT);",
    "  vec3 glow = (uK.x*texture2D(uB0, uvT).rgb + uK.y*texture2D(uB1, uvT).rgb)*(1.0 + 2.2*breath*I)",
    "            + uK.z*texture2D(uB2, uvT).rgb*(1.0 + 2.6*breath*I);",
    "  vec3 mark = logo.rgb*(1.0 + 0.10*breath*I);",
    "  if (sheenOn) {",
    "    float ph = fract(t/4.6); vec2 dir = normalize(vec2(1.0, -0.62));",
    "    float x = dot(p - C, dir) - mix(-230.0, 230.0, ph);",
    "    float s = exp(-x*x/(2.0*18.0*18.0))*smoothstep(0.0, 0.1, ph)*(1.0 - smoothstep(0.9, 1.0, ph));",
    "    mark += vec3(0.80, 0.88, 1.0)*s*smoothstep(0.10, 0.75, luma(logo.rgb))*0.40*min(I, 1.6)*logo.a;",
    "  }",
    "  float db = pulseOn ? 0.5 - 0.5*cos(t*1.7 - 0.9) : 0.3;",
    "  float dD = length(p - uDot.xy) - uDot.z;",
    "  glow += vec3(0.30, 0.55, 1.0)*exp(-max(dD, 0.0)/(3.0 + 7.0*db))*0.30*db*I*step(0.0, dD);",
    "  float edge = smoothstep(0.0, 24.0, p.x)*smoothstep(0.0, 24.0, REF.x - p.x)*smoothstep(0.0, 24.0, p.y)*smoothstep(0.0, 24.0, REF.y - p.y);",
    "  float gA = clamp(max(glow.r, max(glow.g, glow.b)), 0.0, 1.0);",
    "  float a = (logo.a + (1.0 - logo.a)*gA)*edge;",
    "  vec3 rgb = (mark + glow*(1.0 - logo.a))*edge;",
    "  gl_FragColor = vec4(min(rgb, vec3(a)), a);",
    "}"].join("\n");

  function mount(canvas, opts) {
    opts = opts || {};
    var state = { mode: opts.mode == null ? 2 : opts.mode, intensity: opts.intensity == null ? 1 : opts.intensity,
                  speed: opts.speed == null ? 1 : opts.speed, playing: true, t: 0 };
    var reduce = global.matchMedia && global.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) { state.playing = false; state.t = Math.PI/2/1.7; }   // still frame at mid-breath
    var gl = canvas.getContext("webgl", { antialias: false, alpha: true, premultipliedAlpha: true });
    if (!gl) { if (opts.onFail) opts.onFail(); return null; }

    function program(fs) {
      function mk(type, src) { var s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; }
      var p = gl.createProgram();
      gl.attachShader(p, mk(gl.VERTEX_SHADER, VS)); gl.attachShader(p, mk(gl.FRAGMENT_SHADER, fs));
      gl.bindAttribLocation(p, 0, "pos"); gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
      var u = {}, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
      for (var i = 0; i < n; i++) { var a = gl.getActiveUniform(p, i); u[a.name] = gl.getUniformLocation(p, a.name); }
      return { p: p, u: u };
    }
    var P;
    try { P = { field: program(FS_FIELD), dot: program(FS_DOT), blur: program(FS_BLUR), main: program(FS_MAIN) }; }
    catch (e) { console.error(e); if (opts.onFail) opts.onFail(); return null; }
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    function tex(filter) { var t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return t; }
    function target(w, h) { var t = tex(gl.LINEAR);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
      var fb = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t, 0);
      return { t: t, fb: fb, w: w, h: h }; }
    function free(tg) { if (tg) { gl.deleteTexture(tg.t); gl.deleteFramebuffer(tg.fb); } }

    var faces = D.faces.map(function (f) {
      var raw = atob(f.cells), bytes = new Uint8Array(raw.length);
      for (var i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
      var t = tex(gl.NEAREST); gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, f.nx, f.ny, 0, gl.RGBA, gl.UNSIGNED_BYTE, bytes);
      return { cellTex: t, path: new Path2D(f.d), nx: f.nx, ny: f.ny, x0: f.x0, y0: f.y0, h: f.h, s: f.s };
    });
    var maskTex = tex(gl.NEAREST), maskCanvas = document.createElement("canvas");
    function quad() { gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4); }

    function renderMark(tg) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, tg.fb); gl.viewport(0, 0, tg.w, tg.h);
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
      gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      maskCanvas.width = tg.w; maskCanvas.height = tg.h;
      var ctx = maskCanvas.getContext("2d"), u = P.field.u;
      gl.useProgram(P.field.p); gl.uniform2f(u.uView, tg.w, tg.h);
      faces.forEach(function (f) {
        ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, tg.w, tg.h);
        ctx.setTransform(tg.w/REF_W, 0, 0, tg.h/REF_H, 0, 0); ctx.fillStyle = "#fff"; ctx.fill(f.path);
        gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, maskTex);
        gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, maskCanvas);
        gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, f.cellTex);
        gl.uniform1i(u.uCells, 0); gl.uniform1i(u.uMask, 1);
        gl.uniform2f(u.uN, f.nx, f.ny); gl.uniform2f(u.uOrig, f.x0, f.y0); gl.uniform1f(u.uH, f.h); gl.uniform1f(u.uS, f.s);
        quad();
      });
      gl.useProgram(P.dot.p); gl.uniform2f(P.dot.u.uView, tg.w, tg.h);
      gl.uniform3f(P.dot.u.uDot, D.dot[0], D.dot[1], D.dot[2]);
      gl.uniform3f(P.dot.u.uDotCol, D.dotColor[0], D.dotColor[1], D.dotColor[2]);
      quad(); gl.disable(gl.BLEND);
    }
    function blur(src, tmp, dst, sigmaUser) {
      var u = P.blur.u; gl.useProgram(P.blur.p);
      gl.uniform1f(u.uSigma, sigmaUser*src.w/REF_W); gl.uniform1i(u.uSrc, 0); gl.activeTexture(gl.TEXTURE0);
      gl.bindFramebuffer(gl.FRAMEBUFFER, tmp.fb); gl.viewport(0, 0, tmp.w, tmp.h);
      gl.bindTexture(gl.TEXTURE_2D, src.t); gl.uniform2f(u.uView, tmp.w, tmp.h); gl.uniform2f(u.uDir, 1, 0); quad();
      gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fb); gl.viewport(0, 0, dst.w, dst.h);
      gl.bindTexture(gl.TEXTURE_2D, tmp.t); gl.uniform2f(u.uView, dst.w, dst.h); gl.uniform2f(u.uDir, 0, 1); quad();
    }

    var T = {}, ready = false, dead = false, raf = 0, last = performance.now(), resizeTimer = 0;
    function rebuild() {
      var dpr = Math.min(global.devicePixelRatio || 1, 2);
      var w = Math.round(canvas.clientWidth*dpr), h = Math.round(canvas.clientHeight*dpr);
      if (w < 2 || h < 2) return;
      Object.keys(T).forEach(function (k) { free(T[k]); }); T = {};
      T.logo = target(w, h);
      T.small = target(REF_W, REF_H); T.tmp = target(REF_W, REF_H); T.b0 = target(REF_W, REF_H); T.b1 = target(REF_W, REF_H);
      T.tiny = target(REF_W/2, REF_H/2); T.tmp2 = target(REF_W/2, REF_H/2); T.b2 = target(REF_W/2, REF_H/2);
      renderMark(T.logo); renderMark(T.small); renderMark(T.tiny);
      blur(T.small, T.tmp, T.b0, D.sig[0]); blur(T.small, T.tmp, T.b1, D.sig[1]); blur(T.tiny, T.tmp2, T.b2, D.sig[2]);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      canvas.width = w; canvas.height = h; ready = true; draw();
    }
    function draw() {
      if (!ready || dead) return;
      gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
      var m = P.main; gl.useProgram(m.p);
      [T.logo, T.b0, T.b1, T.b2].forEach(function (tg, i) { gl.activeTexture(gl.TEXTURE0 + i); gl.bindTexture(gl.TEXTURE_2D, tg.t); });
      gl.uniform1i(m.u.uLogo, 0); gl.uniform1i(m.u.uB0, 1); gl.uniform1i(m.u.uB1, 2); gl.uniform1i(m.u.uB2, 3);
      gl.uniform3f(m.u.uK, D.k[0], D.k[1], D.k[2]); gl.uniform3f(m.u.uDot, D.dot[0], D.dot[1], D.dot[2]);
      gl.uniform2f(m.u.uRes, canvas.width, canvas.height);
      gl.uniform1f(m.u.uTime, state.t); gl.uniform1f(m.u.uIntensity, state.intensity); gl.uniform1f(m.u.uMode, state.mode);
      quad();
      if (opts.onBreath) {
        var pulsing = state.mode === 0 || state.mode === 2;
        opts.onBreath(state.intensity*(pulsing ? 0.5 - 0.5*Math.cos(state.t*1.7) : 0.3));
      }
    }
    function frame(now) {
      var dt = Math.min((now - last)/1000, 0.1); last = now;
      if (state.playing && !document.hidden) { state.t += dt*state.speed; draw(); }
      raf = requestAnimationFrame(frame);
    }
    var ro = new ResizeObserver(function () { clearTimeout(resizeTimer); resizeTimer = setTimeout(rebuild, 120); });
    ro.observe(canvas);
    rebuild(); raf = requestAnimationFrame(frame);

    return {
      play: function () { if (!reduce) { state.playing = true; last = performance.now(); } },
      pause: function () { state.playing = false; },
      set: function (o) { for (var k in o) if (k in state) state[k] = o[k]; draw(); },
      destroy: function () { dead = true; cancelAnimationFrame(raf); ro.disconnect(); Object.keys(T).forEach(function (k) { free(T[k]); }); }
    };
  }
  global.StemmaLogo = { mount: mount, REF: [REF_W, REF_H] };
})(window);
