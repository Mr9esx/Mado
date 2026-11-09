// LILYGO T-Display-C5 V1.0 wired desktop enclosure. Dimensions in mm.
// PCB: LILYGO STEP, commit f9c43b5. LCD: HD19006C30-V3 drawing.

/* [Print or preview] */
part = "assembled"; // [assembled,base,bezel,button]
tilt = 30;
// Preview the second desk orientation. Print exports always use the same pose.
rest_on_back = false;

/* [USB-C opening] */
// The stock socket opens on the right. Edit these for a right-angle plug.
usb_opening_width = 11.4;
usb_opening_height = 5.4;
usb_opening_start_x = 30.6;

/* [Housing] */
width = 65.05;
face_depth = 29.0;
seam_height = 14.5;
wall = 1.0;
floor = 1.2;
bezel_thickness = 1.0;
// Clearance on each side of the locating lip. Tune on a sample for friction fit.
fit_clearance = 0.14;

/* [Button fit] */
// Radial clearance on each side of a printed shaft.
button_clearance = 0.25;
button_projection = 0.65;

/* [Hidden] */
$fn = 36;
eps = 0.02;
body_center = [0.725, -0.85];
rear_corner_x = 2.2;
rear_corner_y = 2.0;
blend_steps = 36;
rear_landing_y = 16.0;
rear_landing_low = 5.6;
rear_landing_high = 16.3;
lip_depth = 1.35;
lip_wall = 0.85;

// Local Z=0 is the seam. Positive X points to the board's USB-C end.
pcb_top = -3.6;
pcb_bottom = pcb_top - 0.624;
pcb_mounts = [[-27.271, 10.091], [-27.267, -9.921]];
screen_center = [-2.35, 0];
screen_opening = [45.2, 24.1];
button_locations = [[28.026, 8.600], [28.026, -8.636]];
button_shaft = [3.8, 4.4];
button_flange = [5.35, 5.95];
button_tip = pcb_top + 0.9;
button_top = bezel_thickness + button_projection;

module in_desk_frame() {
    translate([0, 0, seam_height])
        rotate([tilt, 0, 0])
            children();
}

module rounded_rectangle(size, radius) {
    offset(r = radius)
        square(size - [2 * radius, 2 * radius], center = true);
}

module footprint(inset = 0) {
    // The cap supplies the rear edge. Leave room for the curve below the face.
    rear_extension = 3;
    translate(body_center + [0, rear_extension / 2])
        square([width - 2 * inset, face_depth + rear_extension - 2 * inset], center = true);
}

function desk_yz(u, v) =
    [u * cos(tilt) - v * sin(tilt), seam_height + u * sin(tilt) + v * cos(tilt)];

// Three collinear controls at each end make the blends tangent to the flat
// face, rear landing and desk, with zero curvature at the joins.
rear_join = desk_yz(body_center.y + face_depth / 2, bezel_thickness);
face_direction = [cos(tilt), sin(tilt)];
upper_controls = [
    rear_join,
    rear_join + 1.7 * face_direction,
    rear_join + 3.4 * face_direction,
    [rear_landing_y, rear_landing_high + 3.6],
    [rear_landing_y, rear_landing_high + 1.9],
    [rear_landing_y, rear_landing_high]
];
lower_controls = [
    [rear_landing_y, rear_landing_low],
    [rear_landing_y, rear_landing_low * 0.75],
    [rear_landing_y, rear_landing_low * 0.50],
    [rear_landing_y - 1.9, 0],
    [rear_landing_y - 3.9, 0],
    [rear_landing_y - 5.8, 0]
];

function smoothstep5(t) =
    let(u = min(max(t, 0), 1))
    u * u * u * (10 - 15 * u + 6 * u * u);

function rear_blend(t, corner_setback, upper) =
    let(s = 1 - t, controls = upper ? upper_controls : lower_controls)
    controls[0] * pow(s, 5) + controls[1] * (5 * pow(s, 4) * t)
    + controls[2] * (10 * pow(s, 3) * t * t)
    + controls[3] * (10 * s * s * pow(t, 3))
    + controls[4] * (5 * s * pow(t, 4)) + controls[5] * pow(t, 5)
    - [corner_setback * (upper ? smoothstep5(t) : 1), 0];

function rear_blend_inset(t, corner_setback, inset, upper) =
    let(
        tangent = rear_blend(min(1, t + 0.0001), corner_setback, upper)
                - rear_blend(max(0, t - 0.0001), corner_setback, upper),
        outward = t == 0 ? (upper ? [-sin(tilt), cos(tilt)] : [1, 0])
                : t == 1 ? (upper ? [1, 0] : [0, -1])
                : [-tangent.y, tangent.x] / norm(tangent)
    )
    rear_blend(t, corner_setback, upper) - inset * outward;

function rear_profile(corner_setback, inset) =
    let(
        front = desk_yz(-22, bezel_thickness).x,
        front_height = seam_height + (bezel_thickness - inset + front * sin(tilt)) / cos(tilt)
    )
    concat(
        [[front, inset]],
        [for (step = [blend_steps:-1:0]) rear_blend_inset(step / blend_steps, corner_setback, inset, false)],
        [for (step = [blend_steps:-1:0]) rear_blend_inset(step / blend_steps, corner_setback, inset, true)],
        [[front, front_height]]
    );

// Roll the two rear corners in plan below the square face.
module rear_envelope(inset = 0) {
    rx = rear_corner_x - inset;
    ry = rear_corner_y - inset;
    left = body_center.x - width / 2 + rear_corner_x;
    right = body_center.x + width / 2 - rear_corner_x;
    sections = concat(
        [for (a = [90:-7.5:0]) [left - rx * sin(a), ry * (1 - cos(a))]],
        [for (a = [0:7.5:90]) [right + rx * sin(a), ry * (1 - cos(a))]]
    );
    ring = 2 * (blend_steps + 1) + 2;
    end = (len(sections) - 1) * ring;
    polyhedron(
        points = [for (section = sections, point = rear_profile(section.y, inset))
            [section.x, point.x, point.y]],
        faces = concat(
            [[for (j = [0:ring - 1]) j]],
            [for (i = [0:len(sections) - 2], j = [0:ring - 1])
                let(a = i * ring + j, b = i * ring + (j + 1) % ring)
                each [[b, a, a + ring], [b, a + ring, b + ring]]],
            [[for (j = [ring - 1:-1:0]) end + j]]
        ),
        convexity = 10
    );
}

module rear_envelope_in_face(inset = 0) {
    rotate([-tilt, 0, 0])
        translate([0, 0, -seam_height])
            rear_envelope(inset);
}

module wedge_prism(inset, bottom) {
    intersection() {
        in_desk_frame()
            translate([0, 0, -50])
                linear_extrude(height = 50)
                    footprint(inset);
        rear_envelope(inset);
        translate([-60, -60, bottom])
            cube([120, 120, 60]);
    }
}

module at_pcb_mounts() {
    for (point = pcb_mounts)
        translate(point)
            children();
}

module at_buttons() {
    for (point = button_locations)
        translate(point)
            children();
}

module support_posts() {
    in_desk_frame() {
        at_pcb_mounts() {
            translate([0, 0, -50])
                cylinder(h = 50 + pcb_bottom, r = 2);
            translate([0, 0, pcb_bottom - eps])
                cylinder(h = 0.56 + eps, d1 = 2.15, d2 = 1.92);
        }

        for (y = [-12, 12]) {
            translate([24, y, -50])
                cylinder(h = 50 + pcb_bottom - 3 + eps, r = 1.4);
            translate([24, y, pcb_bottom - 3])
                cylinder(h = 3, r1 = 1.4, r2 = 0.7);
        }
    }
}

module usb_opening() {
    // The bezel closes the top of the slot, so the base needs no bridge here.
    translate([usb_opening_start_x, 0.13, 0.20 - usb_opening_height / 2])
        rotate([90, 0, 90])
            linear_extrude(height = 12)
                rounded_rectangle([usb_opening_width, usb_opening_height], 0.6);
}

module locating_lip() {
    inset = wall + fit_clearance;
    difference() {
        intersection() {
            // Stop the lip ahead of the rounded rear where the opening narrows.
            translate([-width, -face_depth, -lip_depth])
                cube([2 * width, face_depth + 6.5, lip_depth + eps]);
            translate([0, 0, -lip_depth])
                linear_extrude(height = lip_depth + eps)
                    difference() {
                        footprint(inset);
                        footprint(inset + lip_wall);
                    }
        }
        usb_opening();
    }
}

module base() {
    difference() {
        union() {
            difference() {
                wedge_prism(0, 0);
                wedge_prism(wall, floor);
            }
            intersection() {
                wedge_prism(0, 0);
                support_posts();
            }
        }
        in_desk_frame() usb_opening();
    }
}

module window() {
    translate([screen_center.x, screen_center.y, -eps])
        linear_extrude(height = bezel_thickness + 2 * eps)
            square(screen_opening, center = true);
}

module bezel() {
    difference() {
        union() {
            intersection() {
                rear_envelope_in_face();
                linear_extrude(height = bezel_thickness)
                    footprint();
            }
            at_pcb_mounts()
                translate([0, 0, pcb_top + 0.15])
                    cylinder(h = -pcb_top - 0.15 + eps, r = 1.55);
            for (y = [-12.22, 12.22])
                translate([27.85, y - 0.44, pcb_top + 0.18])
                    cube([1.7, 0.88, -pcb_top - 0.18 + eps]);
            at_buttons()
                translate([0, 0, -0.60])
                    linear_extrude(height = 0.60 + eps)
                        rounded_rectangle([5.85, 6.45], 1.7);
            locating_lip();
        }
        window();
        at_buttons()
            translate([0, 0, -0.62])
                linear_extrude(height = bezel_thickness + 0.62 + eps)
                    rounded_rectangle(button_shaft + 2 * [button_clearance, button_clearance], 1.25);
    }
}

module button() {
    translate([0, 0, -0.78])
        linear_extrude(height = button_top + 0.78)
            rounded_rectangle(button_shaft, 1.0);
    hull() {
        translate([0, 0, -0.78])
            linear_extrude(height = eps)
                rounded_rectangle(button_shaft, 1.0);
        translate([0, 0, -1.55])
            linear_extrude(height = 0.28)
                rounded_rectangle(button_flange, 1.6);
    }
    translate([0, 0, button_tip])
        cylinder(h = -button_tip - 1.55 + eps, r1 = 0.95, r2 = 1.3);
}

assert(tilt > 0 && tilt < 45, "Tilt must be between 0 and 45 degrees.");
assert(wall + fit_clearance < min(rear_corner_x, rear_corner_y),
       "The lip must fit inside the rear corners.");

module assembled() {
    color("#475763") base();
    in_desk_frame() {
        color("#708692") bezel();
        color("#343C40") at_buttons() button();
    }
}

if (part == "base")
    base();
else if (part == "bezel")
    translate([0, 0, bezel_thickness]) rotate([180, 0, 0]) bezel();
else if (part == "button")
    translate([0, 0, button_top]) rotate([180, 0, 0]) button();
else if (rest_on_back)
    translate([0, 0, rear_landing_y]) rotate([-90, 0, 0]) assembled();
else
    assembled();
