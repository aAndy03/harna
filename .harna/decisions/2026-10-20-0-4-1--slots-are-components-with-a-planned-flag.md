# 0.4.1: slots are components with a planned flag

A planned slot is an ordinary manifest component with planned = true rather than a separate register: every view, rule scope, gateway and boundary already understands components, so the future's shape is governed by the same rules from the moment it is declared. Validate treats the missing file (and a slots-only container) as intent; scaffold with the slot's name fills it at its path; once code exists a soft Info asks for harna slot fill instead of flipping silently, because building a slot is a decision worth a line.
