<normalised>
<normalised>
%TF.ProjectId,uniform,756e6966-6f72-46d2-9e6b-696361645f70,rev?*%
%TF.SameCoordinates,Original*%
%TF.FileFunction,Soldermask,Top*%
%TF.FilePolarity,Negative*%
%FSLAX46Y46*%
G04 Gerber Fmt 4.6, Leading zero omitted, Abs format (unit mm)*
<normalised>
%MOMM*%
%LPD*%
G01*
G04 APERTURE LIST*
G04 Aperture macros list*
%AMRoundRect*
0 Rectangle with rounded corners*
0 $1 Rounding radius*
0 $2 $3 $4 $5 $6 $7 $8 $9 X,Y pos of 4 corners*
0 Add a 4 corners polygon primitive as box body*
4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*
0 Add four circle primitives for the rounded corners*
1,1,$1+$1,$2,$3*
1,1,$1+$1,$4,$5*
1,1,$1+$1,$6,$7*
1,1,$1+$1,$8,$9*
0 Add four rect primitives between the rounded corners*
20,1,$1+$1,$2,$3,$4,$5,0*
20,1,$1+$1,$4,$5,$6,$7,0*
20,1,$1+$1,$6,$7,$8,$9,0*
20,1,$1+$1,$8,$9,$2,$3,0*%
%AMHorizOval*
0 Thick line with rounded ends*
0 $1 width*
0 $2 $3 position (X,Y) of the first rounded end (center of the circle)*
0 $4 $5 position (X,Y) of the second rounded end (center of the circle)*
0 Add line between two ends*
20,1,$1,$2,$3,$4,$5,0*
0 Add two circle primitives to create the rounded ends*
1,1,$1,$2,$3*
1,1,$1,$4,$5*%
%AMFreePoly0*
4,1,15,0.109607,0.073607,0.114000,0.063000,0.114000,-0.037800,0.109607,-0.048407,0.084407,-0.073607,0.073800,-0.078000,-0.099000,-0.078000,-0.109607,-0.073607,-0.114000,-0.063000,-0.114000,0.037800,-0.109607,0.048407,-0.084407,0.073607,-0.073800,0.078000,0.099000,0.078000,0.109607,0.073607,0.109607,0.073607,$1*%
%AMFreePoly1*
4,1,17,-0.073359,0.152462,0.106641,0.032462,0.118135,0.017913,0.120000,0.007500,0.120000,-0.007500,0.114271,-0.025134,0.106641,-0.032462,-0.073359,-0.152462,-0.091208,-0.157476,-0.108598,-0.151040,-0.118882,-0.135613,-0.120000,-0.127500,-0.120000,0.127500,-0.114271,0.145134,-0.099271,0.156031,-0.080729,0.156032,-0.073359,0.152462,-0.073359,0.152462,$1*%
%AMFreePoly2*
4,1,13,0.120001,0.107683,0.120893,0.107449,0.130032,0.100503,0.135818,0.090586,0.137840,0.082206,0.137530,0.076541,0.137104,0.073724,0.120001,0.005312,0.120001,-0.120001,-0.120000,-0.120001,-0.120000,0.120000,0.120001,0.120000,0.120001,0.107683,0.120001,0.107683,$1*%
G04 Aperture macros list end*
%ADD10FreePoly0,20.000000*%
%ADD11RoundRect,0.375002X-0.000003X-0.000014X0.000014X-0.000003X0.000003X0.000014X-0.000014X0.000003X0*%
%ADD12RoundRect,0.050000X-0.232583X-0.233142X0.317994X-0.085615X0.232583X0.233142X-0.317994X0.085615X0*%
%ADD13HorizOval,0.460000X0.084854X0.084854X-0.084854X-0.084854X0*%
%ADD14FreePoly1,20.000000*%
%ADD15FreePoly2,20.000000*%
G04 APERTURE END LIST*
D10*
%TO.C,*%
X134571980Y-30993377D03*
D11*
X135326783Y-30503202D03*
D12*
X136081587Y-30013027D03*
D13*
X133918413Y-29986973D03*
D14*
X134673217Y-29496798D03*
D15*
X135428020Y-29006623D03*
%TD*%
M02*
