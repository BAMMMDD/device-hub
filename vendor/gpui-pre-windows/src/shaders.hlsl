#include "alpha_correction.hlsl"

cbuffer GlobalParams: register(b0) {
    float4 gamma_ratios;
    float2 global_viewport_size;
    float grayscale_enhanced_contrast;
    float subpixel_enhanced_contrast;
    uint is_bgr;
    uint3 global_pad;
};

cbuffer BatchParams: register(b1) {
    uint batch_start_index;
    uint3 batch_pad;
};

Texture2D<float4> t_sprite: register(t0);
SamplerState s_sprite: register(s0);

struct SubpixelSpriteFragmentOutput {
    float4 foreground : SV_Target0;
    float4 alpha : SV_Target1;
};

struct Bounds {
    float2 origin;
    float2 size;
};

struct Corners {
    float top_left;
    float top_right;
    float bottom_right;
    float bottom_left;
};

struct Edges {
    float top;
    float right;
    float bottom;
    float left;
};

struct Hsla {
    float h;
    float s;
    float l;
    float a;
};

struct LinearColorStop {
    Hsla color;
    float percentage;
};

struct Background {
    // 0u is Solid
    // 1u is LinearGradient
    // 2u is PatternSlash
    uint tag;
    // 0u is sRGB linear color
    // 1u is Oklab color
    uint color_space;
    Hsla solid;
    float gradient_angle_or_pattern_height;
    LinearColorStop colors[2];
    uint pad;
};

struct GradientColor {
  float4 solid;
  float4 color0;
  float4 color1;
};

struct AtlasTextureId {
    uint index;
    uint kind;
};

struct AtlasBounds {
    int2 origin;
    int2 size;
};

struct AtlasTile {
    AtlasTextureId texture_id;
    uint tile_id;
    uint padding;
    AtlasBounds bounds;
};

struct TransformationMatrix {
    float2x2 rotation_scale;
    float2 translation;
};

static const float M_PI_F = 3.141592653f;
static const float3 GRAYSCALE_FACTORS = float3(0.2126f, 0.7152f, 0.0722f);

float4 to_device_position_impl(float2 position) {
    float2 device_position = position / global_viewport_size * float2(2.0, -2.0) + float2(-1.0, 1.0);
    return float4(device_position, 0., 1.);
}

float4 to_device_position(float2 unit_vertex, Bounds bounds) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    return to_device_position_impl(position);
}

float4 distance_from_clip_rect_impl(float2 position, Bounds clip_bounds) {
    float2 tl = position - clip_bounds.origin;
    float2 br = clip_bounds.origin + clip_bounds.size - position;
    return float4(tl.x, br.x, tl.y, br.y);
}

float4 distance_from_clip_rect(float2 unit_vertex, Bounds bounds, Bounds clip_bounds) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    return distance_from_clip_rect_impl(position, clip_bounds);
}

float4 distance_from_clip_rect_transformed(float2 unit_vertex, Bounds bounds, Bounds clip_bounds, TransformationMatrix transformation) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    float2 transformed = mul(position, transformation.rotation_scale) + transformation.translation;
    return distance_from_clip_rect_impl(transformed, clip_bounds);
}

// Convert linear RGB to sRGB
float3 linear_to_srgb(float3 color) {
    return pow(color, float3(1.0 / 2.2, 1.0 / 2.2, 1.0 / 2.2));
}

// Convert sRGB to linear RGB
float3 srgb_to_linear(float3 color) {
    return pow(color, float3(2.2, 2.2, 2.2));
}

/// Hsla to linear RGBA conversion.
float4 hsla_to_rgba(Hsla hsla) {
    float h = hsla.h * 6.0; // Now, it's an angle but scaled in [0, 6) range
    float s = hsla.s;
    float l = hsla.l;
    float a = hsla.a;

    float c = (1.0 - abs(2.0 * l - 1.0)) * s;
    float x = c * (1.0 - abs(fmod(h, 2.0) - 1.0));
    float m = l - c / 2.0;

    float r = 0.0;
    float g = 0.0;
    float b = 0.0;

    if (h >= 0.0 && h < 1.0) {
        r = c;
        g = x;
        b = 0.0;
    } else if (h >= 1.0 && h < 2.0) {
        r = x;
        g = c;
        b = 0.0;
    } else if (h >= 2.0 && h < 3.0) {
        r = 0.0;
        g = c;
        b = x;
    } else if (h >= 3.0 && h < 4.0) {
        r = 0.0;
        g = x;
        b = c;
    } else if (h >= 4.0 && h < 5.0) {
        r = x;
        g = 0.0;
        b = c;
    } else {
        r = c;
        g = 0.0;
        b = x;
    }

    float4 rgba;
    rgba.x = (r + m);
    rgba.y = (g + m);
    rgba.z = (b + m);
    rgba.w = a;
    return rgba;
}

// Converts a sRGB color to the Oklab color space.
// Reference: https://bottosson.github.io/posts/oklab/#converting-from-linear-srgb-to-oklab
float4 srgb_to_oklab(float4 color) {
    // Convert non-linear sRGB to linear sRGB
    color = float4(srgb_to_linear(color.rgb), color.a);

    float l = 0.4122214708 * color.r + 0.5363325363 * color.g + 0.0514459929 * color.b;
    float m = 0.2119034982 * color.r + 0.6806995451 * color.g + 0.1073969566 * color.b;
    float s = 0.0883024619 * color.r + 0.2817188376 * color.g + 0.6299787005 * color.b;

    float l_ = pow(l, 1.0/3.0);
    float m_ = pow(m, 1.0/3.0);
    float s_ = pow(s, 1.0/3.0);

    return float4(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
        color.a
    );
}

// Converts an Oklab color to the sRGB color space.
float4 oklab_to_srgb(float4 color) {
    float l_ = color.r + 0.3963377774 * color.g + 0.2158037573 * color.b;
    float m_ = color.r - 0.1055613458 * color.g - 0.0638541728 * color.b;
    float s_ = color.r - 0.0894841775 * color.g - 1.2914855480 * color.b;

    float l = l_ * l_ * l_;
    float m = m_ * m_ * m_;
    float s = s_ * s_ * s_;

    float3 linear_rgb = float3(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
    );

    // Convert linear sRGB to non-linear sRGB
    return float4(linear_to_srgb(linear_rgb), color.a);
}

// This approximates the error function, needed for the gaussian integral
float2 erf(float2 x) {
    float2 s = sign(x);
    float2 a = abs(x);
    x = 1. + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    x *= x;
    return s - s / (x * x);
}

float blur_along_x(float x, float y, float sigma, float corner, float2 half_size) {
    float delta = min(half_size.y - corner - abs(y), 0.);
    float curved = half_size.x - corner + sqrt(max(0., corner * corner - delta * delta));
    float2 integral = 0.5 + 0.5 * erf((x + float2(-curved, curved)) * (sqrt(0.5) / sigma));
    return integral.y - integral.x;
}

// A standard gaussian function, used for weighting samples
float gaussian(float x, float sigma) {
    return exp(-(x * x) / (2. * sigma * sigma)) / (sqrt(2. * M_PI_F) * sigma);
}

float4 over(float4 below, float4 above) {
    float4 result;
    float alpha = above.a + below.a * (1.0 - above.a);
    result.rgb = (above.rgb * above.a + below.rgb * below.a * (1.0 - above.a)) / alpha;
    result.a = alpha;
    return result;
}

float2 to_tile_position(float2 unit_vertex, AtlasTile tile) {
    float2 atlas_size;
    t_sprite.GetDimensions(atlas_size.x, atlas_size.y);
    return (float2(tile.bounds.origin) + unit_vertex * float2(tile.bounds.size)) / atlas_size;
}

// Selects corner radius based on quadrant.
float pick_corner_radius(float2 center_to_point, Corners corner_radii) {
    if (center_to_point.x < 0.) {
        if (center_to_point.y < 0.) {
            return corner_radii.top_left;
        } else {
            return corner_radii.bottom_left;
        }
    } else {
        if (center_to_point.y < 0.) {
            return corner_radii.top_right;
        } else {
            return corner_radii.bottom_right;
        }
    }
}

float4 to_device_position_transformed(float2 unit_vertex, Bounds bounds,
                                      TransformationMatrix transformation) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    float2 transformed = mul(position, transformation.rotation_scale) + transformation.translation;
    float2 device_position = transformed / global_viewport_size * float2(2.0, -2.0) + float2(-1.0, 1.0);
    return float4(device_position, 0.0, 1.0);
}

// Implementation of quad signed distance field
float quad_sdf_impl(float2 corner_center_to_point, float corner_radius) {
    if (corner_radius == 0.0) {
        // Fast path for unrounded corners
        return max(corner_center_to_point.x, corner_center_to_point.y);
    } else {
        // Signed distance of the point from a quad that is inset by corner_radius
        // It is negative inside this quad, and positive outside
        float signed_distance_to_inset_quad =
            // 0 inside the inset quad, and positive outside
            length(max(float2(0.0, 0.0), corner_center_to_point)) +
            // 0 outside the inset quad, and negative inside
            min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));

        return signed_distance_to_inset_quad - corner_radius;
    }
}

float quad_sdf(float2 pt, Bounds bounds, Corners corner_radii) {
    float2 half_size = bounds.size / 2.;
    float2 center = bounds.origin + half_size;
    float2 center_to_point = pt - center;
    float corner_radius = pick_corner_radius(center_to_point, corner_radii);
    float2 corner_to_point = abs(center_to_point) - half_size;
    float2 corner_center_to_point = corner_to_point + corner_radius;
    return quad_sdf_impl(corner_center_to_point, corner_radius);
}

GradientColor prepare_gradient_color(uint tag, uint color_space, Hsla solid, LinearColorStop colors[2]) {
    GradientColor output;
    if (tag == 0 || tag == 2 || tag == 3) {
        output.solid = hsla_to_rgba(solid);
    } else if (tag == 1) {
        output.color0 = hsla_to_rgba(colors[0].color);
        output.color1 = hsla_to_rgba(colors[1].color);

        // Prepare color space in vertex for avoid conversion
        // in fragment shader for performance reasons
        if (color_space == 1) {
            // Oklab
            output.color0 = srgb_to_oklab(output.color0);
            output.color1 = srgb_to_oklab(output.color1);
        }
    }

    return output;
}

float2x2 rotate2d(float angle) {
    float s = sin(angle);
    float c = cos(angle);
    return float2x2(c, -s, s, c);
}

float4 gradient_color(Background background,
                      float2 position,
                      Bounds bounds,
                      float4 solid_color, float4 color0, float4 color1) {
    float4 color;

    switch (background.tag) {
        case 0:
            color = solid_color;
            break;
        case 1: {
            // -90 degrees to match the CSS gradient angle.
            float gradient_angle = background.gradient_angle_or_pattern_height;
            float radians = (fmod(gradient_angle, 360.0) - 90.0) * (M_PI_F / 180.0);
            float2 direction = float2(cos(radians), sin(radians));

            // Expand the short side to be the same as the long side
            if (bounds.size.x > bounds.size.y) {
                direction.y *= bounds.size.y / bounds.size.x;
            } else {
                direction.x *=  bounds.size.x / bounds.size.y;
            }

            // Get the t value for the linear gradient with the color stop percentages.
            float2 half_size = bounds.size * 0.5;
            float2 center = bounds.origin + half_size;
            float2 center_to_point = position - center;
            float t = dot(center_to_point, direction) / length(direction);
            // Check the direct to determine the use x or y
            if (abs(direction.x) > abs(direction.y)) {
                t = (t + half_size.x) / bounds.size.x;
            } else {
                t = (t + half_size.y) / bounds.size.y;
            }

            // Adjust t based on the stop percentages
            t = (t - background.colors[0].percentage)
                / (background.colors[1].percentage
                - background.colors[0].percentage);
            t = clamp(t, 0.0, 1.0);

            switch (background.color_space) {
                case 0:
                    color = lerp(color0, color1, t);
                    break;
                case 1: {
                    float4 oklab_color = lerp(color0, color1, t);
                    color = oklab_to_srgb(oklab_color);
                    break;
                }
            }

            // Dither to reduce banding in gradients (especially dark/alpha).
            // Triangular-distributed noise breaks up 8-bit quantization steps.
            // ±2/255 for RGB (enough for dark-on-dark compositing),
            // ±3/255 for alpha (needs more because alpha × dark color = tiny steps).
            {
                float2 seed = position * 0.6180339887; // golden ratio spread
                float r1 = frac(sin(dot(seed, float2(12.9898, 78.233))) * 43758.5453);
                float r2 = frac(sin(dot(seed, float2(39.3460, 11.135))) * 24634.6345);
                float tri = r1 + r2 - 1.0; // triangular PDF, range [-1, +1]
                color.rgb += tri * 2.0 / 255.0;
                color.a   += tri * 3.0 / 255.0;
            }

            break;
        }
        case 2: {
            float gradient_angle_or_pattern_height = background.gradient_angle_or_pattern_height;
            float pattern_width = (gradient_angle_or_pattern_height / 65535.0f) / 255.0f;
            float pattern_interval = fmod(gradient_angle_or_pattern_height, 65535.0f) / 255.0f;
            float pattern_height = pattern_width + pattern_interval;
            float stripe_angle = M_PI_F / 4.0;
            float pattern_period = pattern_height * sin(stripe_angle);
            float2x2 rotation = rotate2d(stripe_angle);
            float2 relative_position = position - bounds.origin;
            float2 rotated_point = mul(relative_position, rotation);
            float pattern = fmod(rotated_point.x, pattern_period);
            float distance = min(pattern, pattern_period - pattern) - pattern_period * (pattern_width / pattern_height) /  2.0f;
            color = solid_color;
            color.a *= saturate(0.5 - distance);
            break;
        }
        case 3: {
            // checkerboard
            float size = background.gradient_angle_or_pattern_height;
            float2 relative_position = position - bounds.origin;

            float x_index = floor(relative_position.x / size);
            float y_index = floor(relative_position.y / size);
            float should_be_colored = (x_index + y_index) % 2.0;

            color = solid_color;
            color.a *= saturate(should_be_colored);
            break;
        }
    }

    return color;
}

// Returns the dash velocity of a corner given the dash velocity of the two
// sides, by returning the slower velocity (larger dashes).
//
// Since 0 is used for dash velocity when the border width is 0 (instead of
// +inf), this returns the other dash velocity in that case.
//
// An alternative to this might be to appropriately interpolate the dash
// velocity around the corner, but that seems overcomplicated.
float corner_dash_velocity(float dv1, float dv2) {
    if (dv1 == 0.0) {
        return dv2;
    } else if (dv2 == 0.0) {
        return dv1;
    } else {
        return min(dv1, dv2);
    }
}

// Returns alpha used to render antialiased dashes.
// `t` is within the dash when `fmod(t, period) < length`.
float dash_alpha(
    float t, float period, float length, float dash_velocity,
    float antialias_threshold
) {
    float half_period = period / 2.0;
    float half_length = length / 2.0;
    // Value in [-half_period, half_period]
    // The dash is in [-half_length, half_length]
    float centered = fmod(t + half_period - half_length, period) - half_period;
    // Signed distance for the dash, negative values are inside the dash
    float signed_distance = abs(centered) - half_length;
    // Antialiased alpha based on the signed distance
    return saturate(antialias_threshold - signed_distance / dash_velocity);
}

// This approximates distance to the nearest point to a quarter ellipse in a way
// that is sufficient for anti-aliasing when the ellipse is not very eccentric.
// The components of `point` are expected to be positive.
//
// Negative on the outside and positive on the inside.
float quarter_ellipse_sdf(float2 pt, float2 radii) {
    // Scale the space to treat the ellipse like a unit circle
    float2 circle_vec = pt / radii;
    float unit_circle_sdf = length(circle_vec) - 1.0;
    // Approximate up-scaling of the length by using the average of the radii.
    //
    // TODO: A better solution would be to use the gradient of the implicit
    // function for an ellipse to approximate a scaling factor.
    return unit_circle_sdf * (radii.x + radii.y) * -0.5;
}

/*
**
**              Quads
**
*/

struct Quad {
    uint order;
    uint border_style;
    Bounds bounds;
    Bounds content_mask;
    Background background;
    Hsla border_color;
    Corners corner_radii;
    Edges border_widths;
};

struct QuadVertexOutput {
    nointerpolation uint quad_id: TEXCOORD0;
    float4 position: SV_Position;
    nointerpolation float4 border_color: COLOR0;
    nointerpolation float4 background_solid: COLOR1;
    nointerpolation float4 background_color0: COLOR2;
    nointerpolation float4 background_color1: COLOR3;
    float4 clip_distance: SV_ClipDistance;
};

struct QuadFragmentInput {
    nointerpolation uint quad_id: TEXCOORD0;
    float4 position: SV_Position;
    nointerpolation float4 border_color: COLOR0;
    nointerpolation float4 background_solid: COLOR1;
    nointerpolation float4 background_color0: COLOR2;
    nointerpolation float4 background_color1: COLOR3;
};

StructuredBuffer<Quad> quads: register(t1);

QuadVertexOutput quad_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint quad_id = batch_start_index + instance_id;
    Quad quad = quads[quad_id];
    float4 device_position = to_device_position(unit_vertex, quad.bounds);

    GradientColor gradient = prepare_gradient_color(
        quad.background.tag,
        quad.background.color_space,
        quad.background.solid,
        quad.background.colors
    );
    float4 clip_distance = distance_from_clip_rect(unit_vertex, quad.bounds, quad.content_mask);
    float4 border_color = hsla_to_rgba(quad.border_color);

    QuadVertexOutput output;
    output.position = device_position;
    output.border_color = border_color;
    output.quad_id = quad_id;
    output.background_solid = gradient.solid;
    output.background_color0 = gradient.color0;
    output.background_color1 = gradient.color1;
    output.clip_distance = clip_distance;
    return output;
}

float4 quad_fragment(QuadFragmentInput input): SV_Target {
    Quad quad = quads[input.quad_id];
    float4 background_color = gradient_color(quad.background, input.position.xy, quad.bounds,
    input.background_solid, input.background_color0, input.background_color1);

    bool unrounded = quad.corner_radii.top_left == 0.0 &&
        quad.corner_radii.top_right == 0.0 &&
        quad.corner_radii.bottom_left == 0.0 &&
        quad.corner_radii.bottom_right == 0.0;

    // Fast path when the quad is not rounded and doesn't have any border
    if (quad.border_widths.top == 0.0 &&
        quad.border_widths.left == 0.0 &&
        quad.border_widths.right == 0.0 &&
        quad.border_widths.bottom == 0.0 &&
        unrounded) {
        return background_color;
    }

    float2 size = quad.bounds.size;
    float2 half_size = size / 2.;
    float2 the_point = input.position.xy - quad.bounds.origin;
    float2 center_to_point = the_point - half_size;

    // Signed distance field threshold for inclusion of pixels. 0.5 is the
    // minimum distance between the center of the pixel and the edge.
    const float antialias_threshold = 0.5;

    // Radius of the nearest corner
    float corner_radius = pick_corner_radius(center_to_point, quad.corner_radii);

    float2 border = float2(
        center_to_point.x < 0.0 ? quad.border_widths.left : quad.border_widths.right,
        center_to_point.y < 0.0 ? quad.border_widths.top : quad.border_widths.bottom
    );

    // 0-width borders are reduced so that `inner_sdf >= antialias_threshold`.
    // The purpose of this is to not draw antialiasing pixels in this case.
    float2 reduced_border = float2(
        border.x == 0.0 ? -antialias_threshold : border.x,
        border.y == 0.0 ? -antialias_threshold : border.y
    );

    // Vector from the corner of the quad bounds to the point, after mirroring
    // the point into the bottom right quadrant. Both components are <= 0.
    float2 corner_to_point = abs(center_to_point) - half_size;

    // Vector from the point to the center of the rounded corner's circle, also
    // mirrored into bottom right quadrant.
    float2 corner_center_to_point = corner_to_point + corner_radius;

    // Whether the nearest point on the border is rounded
    bool is_near_rounded_corner =
        corner_center_to_point.x >= 0.0 &&
        corner_center_to_point.y >= 0.0;

    // Vector from straight border inner corner to point.
    //
    // 0-width borders are turned into width -1 so that inner_sdf is > 1.0 near
    // the border. Without this, antialiasing pixels would be drawn.
    float2 straight_border_inner_corner_to_point = corner_to_point + reduced_border;

    // Whether the point is beyond the inner edge of the straight border
    bool is_beyond_inner_straight_border =
        straight_border_inner_corner_to_point.x > 0.0 ||
        straight_border_inner_corner_to_point.y > 0.0;

    // Whether the point is far enough inside the quad, such that the pixels are
    // not affected by the straight border.
    bool is_within_inner_straight_border =
        straight_border_inner_corner_to_point.x < -antialias_threshold &&
        straight_border_inner_corner_to_point.y < -antialias_threshold;

    // Fast path for points that must be part of the background
    if (is_within_inner_straight_border && !is_near_rounded_corner) {
        return background_color;
    }

    // Signed distance of the point to the outside edge of the quad's border
    float outer_sdf = quad_sdf_impl(corner_center_to_point, corner_radius);

    // Approximate signed distance of the point to the inside edge of the quad's
    // border. It is negative outside this edge (within the border), and
    // positive inside.
    //
    // This is not always an accurate signed distance:
    // * The rounded portions with varying border width use an approximation of
    //   nearest-point-on-ellipse.
    // * When it is quickly known to be outside the edge, -1.0 is used.
    float inner_sdf = 0.0;
    if (corner_center_to_point.x <= 0.0 || corner_center_to_point.y <= 0.0) {
        // Fast paths for straight borders
        inner_sdf = -max(straight_border_inner_corner_to_point.x,
                        straight_border_inner_corner_to_point.y);
    } else if (is_beyond_inner_straight_border) {
        // Fast path for points that must be outside the inner edge
        inner_sdf = -1.0;
    } else if (reduced_border.x == reduced_border.y) {
        // Fast path for circular inner edge.
        inner_sdf = -(outer_sdf + reduced_border.x);
    } else {
        float2 ellipse_radii = max(float2(0.0, 0.0), float2(corner_radius, corner_radius) - reduced_border);
        inner_sdf = quarter_ellipse_sdf(corner_center_to_point, ellipse_radii);
    }

    // Negative when inside the border
    float border_sdf = max(inner_sdf, outer_sdf);

    float4 color = background_color;
    if (border_sdf < antialias_threshold) {
        float4 border_color = input.border_color;
        // Dashed border logic when border_style == 1
        if (quad.border_style == 1) {
            // Position along the perimeter in "dash space", where each dash
            // period has length 1
            float t = 0.0;

            // Total number of dash periods, so that the dash spacing can be
            // adjusted to evenly divide it
            float max_t = 0.0;

            // Border width is proportional to dash size. This is the behavior
            // used by browsers, but also avoids dashes from different segments
            // overlapping when dash size is smaller than the border width.
            //
            // Dash pattern: (2 * border width) dash, (1 * border width) gap
            const float dash_length_per_width = 2.0;
            const float dash_gap_per_width = 1.0;
            const float dash_period_per_width = dash_length_per_width + dash_gap_per_width;

            // Since the dash size is determined by border width, the density of
            // dashes varies. Multiplying a pixel distance by this returns a
            // position in dash space - it has units (dash period / pixels). So
            // a dash velocity of (1 / 10) is 1 dash every 10 pixels.
            float dash_velocity = 0.0;

            // Dividing this by the border width gives the dash velocity
            const float dv_numerator = 1.0 / dash_period_per_width;

            if (unrounded) {
                // When corners aren't rounded, the dashes are separately laid
                // out on each straight line, rather than around the whole
                // perimeter. This way each line starts and ends with a dash.
                bool is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
                // Choosing the right border width for dashed borders.
                // TODO: A better solution exists taking a look at the whole file.
                // this does not fix single dashed borders at the corners
                float2 dashed_border = float2(
                    max(quad.border_widths.bottom, quad.border_widths.top),
                    max(quad.border_widths.right, quad.border_widths.left)
                );
                float border_width = is_horizontal ? dashed_border.x : dashed_border.y;
                dash_velocity = dv_numerator / border_width;
                t = is_horizontal ? the_point.x : the_point.y;
                t *= dash_velocity;
                max_t = is_horizontal ? size.x : size.y;
                max_t *= dash_velocity;
            } else {
                // When corners are rounded, the dashes are laid out clockwise
                // around the whole perimeter.

                float r_tr = quad.corner_radii.top_right;
                float r_br = quad.corner_radii.bottom_right;
                float r_bl = quad.corner_radii.bottom_left;
                float r_tl = quad.corner_radii.top_left;

                float w_t = quad.border_widths.top;
                float w_r = quad.border_widths.right;
                float w_b = quad.border_widths.bottom;
                float w_l = quad.border_widths.left;

                // Straight side dash velocities
                float dv_t = w_t <= 0.0 ? 0.0 : dv_numerator / w_t;
                float dv_r = w_r <= 0.0 ? 0.0 : dv_numerator / w_r;
                float dv_b = w_b <= 0.0 ? 0.0 : dv_numerator / w_b;
                float dv_l = w_l <= 0.0 ? 0.0 : dv_numerator / w_l;

                // Straight side lengths in dash space
                float s_t = (size.x - r_tl - r_tr) * dv_t;
                float s_r = (size.y - r_tr - r_br) * dv_r;
                float s_b = (size.x - r_br - r_bl) * dv_b;
                float s_l = (size.y - r_bl - r_tl) * dv_l;

                float corner_dash_velocity_tr = corner_dash_velocity(dv_t, dv_r);
                float corner_dash_velocity_br = corner_dash_velocity(dv_b, dv_r);
                float corner_dash_velocity_bl = corner_dash_velocity(dv_b, dv_l);
                float corner_dash_velocity_tl = corner_dash_velocity(dv_t, dv_l);

                // Corner lengths in dash space
                float c_tr = r_tr * (M_PI_F / 2.0) * corner_dash_velocity_tr;
                float c_br = r_br * (M_PI_F / 2.0) * corner_dash_velocity_br;
                float c_bl = r_bl * (M_PI_F / 2.0) * corner_dash_velocity_bl;
                float c_tl = r_tl * (M_PI_F / 2.0) * corner_dash_velocity_tl;

                // Cumulative dash space upto each segment
                float upto_tr = s_t;
                float upto_r = upto_tr + c_tr;
                float upto_br = upto_r + s_r;
                float upto_b = upto_br + c_br;
                float upto_bl = upto_b + s_b;
                float upto_l = upto_bl + c_bl;
                float upto_tl = upto_l + s_l;
                max_t = upto_tl + c_tl;

                if (is_near_rounded_corner) {
                    float radians = atan2(corner_center_to_point.y, corner_center_to_point.x);
                    float corner_t = radians * corner_radius;

                    if (center_to_point.x >= 0.0) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = corner_dash_velocity_tr;
                            // Subtracted because radians is pi/2 to 0 when
                            // going clockwise around the top right corner,
                            // since the y axis has been flipped
                            t = upto_r - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_br;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the bottom-right corner
                            t = upto_br + corner_t * dash_velocity;
                        }
                    } else {
                        if (center_to_point.y >= 0.0) {
                            dash_velocity = corner_dash_velocity_bl;
                            // Subtracted because radians is pi/1 to 0 when
                            // going clockwise around the bottom-left corner,
                            // since the x axis has been flipped
                            t = upto_l - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_tl;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the top-left corner, since both
                            // axis were flipped
                            t = upto_tl + corner_t * dash_velocity;
                        }
                    }
                } else {
                    // Straight borders
                    bool is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
                    if (is_horizontal) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = dv_t;
                            t = (the_point.x - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_b;
                            t = upto_bl - (the_point.x - r_bl) * dash_velocity;
                        }
                    } else {
                        if (center_to_point.x < 0.0) {
                            dash_velocity = dv_l;
                            t = upto_tl - (the_point.y - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_r;
                            t = upto_r + (the_point.y - r_tr) * dash_velocity;
                        }
                    }
                }
            }
            float dash_length = dash_length_per_width / dash_period_per_width;
            float desired_dash_gap = dash_gap_per_width / dash_period_per_width;

            // Straight borders should start and end with a dash, so max_t is
            // reduced to cause this.
            max_t -= unrounded ? dash_length : 0.0;
            if (max_t >= 1.0) {
                // Adjust dash gap to evenly divide max_t
                float dash_count = floor(max_t);
                float dash_period = max_t / dash_count;
                border_color.a *= dash_alpha(t, dash_period, dash_length, dash_velocity, antialias_threshold);
            } else if (unrounded) {
                // When there isn't enough space for the full gap between the
                // two start / end dashes of a straight border, reduce gap to
                // make them fit.
                float dash_gap = max_t - dash_length;
                if (dash_gap > 0.0) {
                    float dash_period = dash_length + dash_gap;
                    border_color.a *= dash_alpha(t, dash_period, dash_length, dash_velocity, antialias_threshold);
                }
            }
        }

        // Blend the border on top of the background and then linearly interpolate
        // between the two as we slide inside the background.
        float4 blended_border = over(background_color, border_color);
        color = lerp(background_color, blended_border,
                    saturate(antialias_threshold - inner_sdf));
    }

    return color * float4(1.0, 1.0, 1.0, saturate(antialias_threshold - outer_sdf));
}

/*
**
**              Shadows
**
*/

struct Shadow {
    uint order;
    float blur_radius;
    Bounds bounds;
    Corners corner_radii;
    Bounds content_mask;
    Hsla color;
    Bounds element_bounds;
    Corners element_corner_radii;
    uint inset;
    uint pad; // align to 8 bytes
};

struct ShadowVertexOutput {
    nointerpolation uint shadow_id: TEXCOORD0;
    float4 position: SV_Position;
    nointerpolation float4 color: COLOR;
    float4 clip_distance: SV_ClipDistance;
};

struct ShadowFragmentInput {
  nointerpolation uint shadow_id: TEXCOORD0;
  float4 position: SV_Position;
  nointerpolation float4 color: COLOR;
};

StructuredBuffer<Shadow> shadows: register(t1);

ShadowVertexOutput shadow_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint shadow_id = batch_start_index + instance_id;
    Shadow shadow = shadows[shadow_id];

    Bounds bounds;
    if (shadow.inset != 0u) {
        bounds = shadow.element_bounds;
    } else {
        // Leave room for the gaussian tail outside the shadow rect.
        float margin = 3.0 * shadow.blur_radius;
        bounds = shadow.bounds;
        bounds.origin -= margin;
        bounds.size += 2.0 * margin;
    }

    float4 device_position = to_device_position(unit_vertex, bounds);
    float4 clip_distance = distance_from_clip_rect(unit_vertex, bounds, shadow.content_mask);
    float4 color = hsla_to_rgba(shadow.color);

    ShadowVertexOutput output;
    output.position = device_position;
    output.color = color;
    output.shadow_id = shadow_id;
    output.clip_distance = clip_distance;

    return output;
}

float4 shadow_fragment(ShadowFragmentInput input): SV_TARGET {
    Shadow shadow = shadows[input.shadow_id];

    float2 half_size = shadow.bounds.size / 2.;
    float2 center = shadow.bounds.origin + half_size;
    float2 point0 = input.position.xy - center;
    float corner_radius = pick_corner_radius(point0, shadow.corner_radii);

    float alpha;
    if (shadow.blur_radius == 0.) {
        float distance = quad_sdf(input.position.xy, shadow.bounds, shadow.corner_radii);
        alpha = saturate(0.5 - distance);
    } else {
        // The signal is only non-zero in a limited range, so don't waste samples
        float low = point0.y - half_size.y;
        float high = point0.y + half_size.y;
        float start = clamp(-3. * shadow.blur_radius, low, high);
        float end = clamp(3. * shadow.blur_radius, low, high);

        // Accumulate samples (we can get away with surprisingly few samples)
        float step = (end - start) / 4.;
        float y = start + step * 0.5;
        alpha = 0.;
        for (int i = 0; i < 4; i++) {
            alpha += blur_along_x(point0.x, point0.y - y, shadow.blur_radius,
                                corner_radius, half_size) *
                    gaussian(y, shadow.blur_radius) * step;
            y += step;
        }
    }

    if (shadow.inset != 0u) {
        // The inset shadow is the complement of the (blurred) hole rect, clipped to the element.
        // `saturate(0.5 - d)` gives a 1-pixel antialiased edge: d <= -0.5 -> 1, d >= 0.5 -> 0.
        alpha = 1.0 - alpha;
        float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
                                          shadow.element_corner_radii);
        alpha *= saturate(0.5 - element_distance);
    } else {
        // A drop shadow belongs outside its element, including when the element
        // has a translucent backdrop material. Without the cutout the glass
        // samples its own solid shadow and white surfaces turn into gray slabs.
        float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
                                          shadow.element_corner_radii);
        alpha *= saturate(0.5 + element_distance);
    }

    return input.color * float4(1., 1., 1., alpha);
}

/*
**
**              Path Rasterization
**
*/

struct PathRasterizationSprite {
    float2 xy_position;
    float2 st_position;
    Background color;
    Bounds bounds;
};

StructuredBuffer<PathRasterizationSprite> path_rasterization_sprites: register(t1);

struct PathVertexOutput {
    float4 position: SV_Position;
    float2 st_position: TEXCOORD0;
    nointerpolation uint vertex_id: TEXCOORD1;
    float4 clip_distance: SV_ClipDistance;
};

struct PathFragmentInput {
    float4 position: SV_Position;
    float2 st_position: TEXCOORD0;
    nointerpolation uint vertex_id: TEXCOORD1;
};

PathVertexOutput path_rasterization_vertex(uint vertex_id: SV_VertexID) {
    PathRasterizationSprite sprite = path_rasterization_sprites[vertex_id];

    PathVertexOutput output;
    output.position = to_device_position_impl(sprite.xy_position);
    output.st_position = sprite.st_position;
    output.vertex_id = vertex_id;
    output.clip_distance = distance_from_clip_rect_impl(sprite.xy_position, sprite.bounds);

    return output;
}

float4 path_rasterization_fragment(PathFragmentInput input): SV_Target {
    float2 dx = ddx(input.st_position);
    float2 dy = ddy(input.st_position);
    PathRasterizationSprite sprite = path_rasterization_sprites[input.vertex_id];

    Background background = sprite.color;
    Bounds bounds = sprite.bounds;

    float alpha;
    if (length(float2(dx.x, dy.x))) {
        alpha = 1.0;
    } else {
        float2 gradient = 2.0 * input.st_position.xx * float2(dx.x, dy.x) - float2(dx.y, dy.y);
        float f = input.st_position.x * input.st_position.x - input.st_position.y;
        float distance = f / length(gradient);
        alpha = saturate(0.5 - distance);
    }

    GradientColor gradient = prepare_gradient_color(
        background.tag, background.color_space, background.solid, background.colors);

    float4 color = gradient_color(background, input.position.xy, bounds,
        gradient.solid, gradient.color0, gradient.color1);
    return float4(color.rgb * color.a * alpha, alpha * color.a);
}

/*
**
**              Path Sprites
**
*/

struct PathSprite {
    Bounds bounds;
};

struct PathSpriteVertexOutput {
    float4 position: SV_Position;
    float2 texture_coords: TEXCOORD0;
};

StructuredBuffer<PathSprite> path_sprites: register(t1);

PathSpriteVertexOutput path_sprite_vertex(uint vertex_id: SV_VertexID, uint sprite_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    PathSprite sprite = path_sprites[sprite_id];

    // Don't apply content mask because it was already accounted for when rasterizing the path
    float4 device_position = to_device_position(unit_vertex, sprite.bounds);

    float2 screen_position = sprite.bounds.origin + unit_vertex * sprite.bounds.size;
    float2 texture_coords = screen_position / global_viewport_size;

    PathSpriteVertexOutput output;
    output.position = device_position;
    output.texture_coords = texture_coords;
    return output;
}

float4 path_sprite_fragment(PathSpriteVertexOutput input): SV_Target {
    return t_sprite.Sample(s_sprite, input.texture_coords);
}

/*
**
**              Underlines
**
*/

struct Underline {
    uint order;
    uint pad;
    Bounds bounds;
    Bounds content_mask;
    Hsla color;
    float thickness;
    uint wavy;
};

struct UnderlineVertexOutput {
  nointerpolation uint underline_id: TEXCOORD0;
  float4 position: SV_Position;
  nointerpolation float4 color: COLOR;
  float4 clip_distance: SV_ClipDistance;
};

struct UnderlineFragmentInput {
  nointerpolation uint underline_id: TEXCOORD0;
  float4 position: SV_Position;
  nointerpolation float4 color: COLOR;
};

StructuredBuffer<Underline> underlines: register(t1);

UnderlineVertexOutput underline_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint underline_id = batch_start_index + instance_id;
    Underline underline = underlines[underline_id];
    float4 device_position = to_device_position(unit_vertex, underline.bounds);
    float4 clip_distance = distance_from_clip_rect(unit_vertex, underline.bounds,
                                                    underline.content_mask);
    float4 color = hsla_to_rgba(underline.color);

    UnderlineVertexOutput output;
    output.position = device_position;
    output.color = color;
    output.underline_id = underline_id;
    output.clip_distance = clip_distance;
    return output;
}

float4 underline_fragment(UnderlineFragmentInput input): SV_Target {
    const float WAVE_FREQUENCY = 2.0;
    const float WAVE_HEIGHT_RATIO = 0.8;

    Underline underline = underlines[input.underline_id];
    if (underline.wavy) {
        float half_thickness = underline.thickness * 0.5;
        float2 origin = underline.bounds.origin;

        float2 st = ((input.position.xy - origin) / underline.bounds.size.y) - float2(0., 0.5);
        float frequency = (M_PI_F * WAVE_FREQUENCY * underline.thickness) / underline.bounds.size.y;
        float amplitude = (underline.thickness * WAVE_HEIGHT_RATIO) / underline.bounds.size.y;

        float sine = sin(st.x * frequency) * amplitude;
        float dSine = cos(st.x * frequency) * amplitude * frequency;
        float distance = (st.y - sine) / sqrt(1. + dSine * dSine);
        float distance_in_pixels = distance * underline.bounds.size.y;
        float distance_from_top_border = distance_in_pixels - half_thickness;
        float distance_from_bottom_border = distance_in_pixels + half_thickness;
        float alpha = saturate(
            0.5 - max(-distance_from_bottom_border, distance_from_top_border));
        return input.color * float4(1., 1., 1., alpha);
    } else {
        return input.color;
    }
}

/*
**
**              Monochrome sprites
**
*/

struct MonochromeSprite {
    uint order;
    uint pad;
    AtlasTile tile;
    Bounds bounds;
    Bounds content_mask;
    Hsla color;
    TransformationMatrix transformation;
};

struct MonochromeSpriteVertexOutput {
    float4 position: SV_Position;
    float2 tile_position: POSITION;
    nointerpolation float4 color: COLOR;
    float4 clip_distance: SV_ClipDistance;
};

struct MonochromeSpriteFragmentInput {
    float4 position: SV_Position;
    float2 tile_position: POSITION;
    nointerpolation float4 color: COLOR;
    float4 clip_distance: SV_ClipDistance;
};

StructuredBuffer<MonochromeSprite> mono_sprites: register(t1);

MonochromeSpriteVertexOutput monochrome_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint sprite_id = batch_start_index + instance_id;
    MonochromeSprite sprite = mono_sprites[sprite_id];
    float4 device_position =
        to_device_position_transformed(unit_vertex, sprite.bounds, sprite.transformation);
    float4 clip_distance = distance_from_clip_rect_transformed(unit_vertex, sprite.bounds, sprite.content_mask, sprite.transformation);
    float2 tile_position = to_tile_position(unit_vertex, sprite.tile);
    float4 color = hsla_to_rgba(sprite.color);

    MonochromeSpriteVertexOutput output;
    output.position = device_position;
    output.tile_position = tile_position;
    output.color = color;
    output.clip_distance = clip_distance;
    return output;
}

float4 monochrome_sprite_fragment(MonochromeSpriteFragmentInput input): SV_Target {
    float sample = t_sprite.Sample(s_sprite, input.tile_position).r;
    float alpha_corrected = apply_contrast_and_gamma_correction(sample, input.color.rgb, grayscale_enhanced_contrast, gamma_ratios);
    return float4(input.color.rgb, input.color.a * alpha_corrected);
}

MonochromeSpriteVertexOutput subpixel_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    return monochrome_sprite_vertex(vertex_id, instance_id);
}

SubpixelSpriteFragmentOutput subpixel_sprite_fragment(MonochromeSpriteFragmentInput input) {
    float3 sample = t_sprite.Sample(s_sprite, input.tile_position).rgb;
    if (is_bgr) {
        sample = sample.bgr;
    }
    float3 alpha_corrected = apply_contrast_and_gamma_correction3(sample, input.color.rgb, subpixel_enhanced_contrast, gamma_ratios);

    SubpixelSpriteFragmentOutput output;
    output.foreground = float4(input.color.rgb, 1.0f);
    output.alpha = float4(input.color.a * alpha_corrected, 1.0f);
    return output;
}

/*
**
**              Polychrome sprites
**
*/

struct PolychromeSprite {
    uint order;
    uint pad;
    AtlasTile tile;
    uint grayscale;
    float opacity;
    Bounds bounds;
    Bounds content_mask;
    Corners corner_radii;
};

struct PolychromeSpriteVertexOutput {
    nointerpolation uint sprite_id: TEXCOORD0;
    float4 position: SV_Position;
    float2 tile_position: POSITION;
    float4 clip_distance: SV_ClipDistance;
};

struct PolychromeSpriteFragmentInput {
    nointerpolation uint sprite_id: TEXCOORD0;
    float4 position: SV_Position;
    float2 tile_position: POSITION;
};

StructuredBuffer<PolychromeSprite> poly_sprites: register(t1);

PolychromeSpriteVertexOutput polychrome_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint sprite_id = batch_start_index + instance_id;
    PolychromeSprite sprite = poly_sprites[sprite_id];
    float4 device_position = to_device_position(unit_vertex, sprite.bounds);
    float4 clip_distance = distance_from_clip_rect(unit_vertex, sprite.bounds,
                                                    sprite.content_mask);
    float2 tile_position = to_tile_position(unit_vertex, sprite.tile);

    PolychromeSpriteVertexOutput output;
    output.position = device_position;
    output.tile_position = tile_position;
    output.sprite_id = sprite_id;
    output.clip_distance = clip_distance;
    return output;
}

float4 polychrome_sprite_fragment(PolychromeSpriteFragmentInput input): SV_Target {
    PolychromeSprite sprite = poly_sprites[input.sprite_id];
    float4 sample = t_sprite.Sample(s_sprite, input.tile_position);
    float distance = quad_sdf(input.position.xy, sprite.bounds, sprite.corner_radii);

    float4 color = sample;
    if (sprite.grayscale != 0u) {
        float3 grayscale = dot(color.rgb, GRAYSCALE_FACTORS);
        color = float4(grayscale, sample.a);
    }
    color.a *= sprite.opacity * saturate(0.5 - distance);
    return color;
}

/*
**
**              DEVHUB: Backdrop filter (Liquid Glass)
**
** A backdrop batch copies the render target into `t_sprite`, shrinks it with
** `backdrop_down` (a 5-tap dual-filter downsample per level), blurs the last
** level with the separable `backdrop_blur`, and then `backdrop` composites each
** region: refraction at the rim, saturate(), brightness(), rim lighting.
** Every pass reads only inside `bd_src_clamp` so stale texels from earlier
** frames never leak in.
**
*/

cbuffer BackdropPassParams: register(b2) {
    // Destination rectangle (pixels of the target) and the target's size.
    float4 bd_dst_rect;
    float2 bd_dst_size;
    // Source texture size and the texel rectangle reads are clamped to (min.xy, max.zw).
    float2 bd_src_size;
    float4 bd_src_clamp;
    // Blur direction in source texels, sigma in texels, tap radius.
    float2 bd_dir;
    float bd_sigma;
    float bd_radius;
    // Composite: device pixels per blurred texel.
    float bd_level_scale;
    float bd_pad;
    // Texel position of the source's (0, 0): non-zero for a region-sized copy.
    float2 bd_src_origin;
    float2 bd_sharp_size;
    float2 bd_sharp_origin;
    float4 bd_sharp_clamp;
};

SamplerState s_backdrop: register(s1);
Texture2D<float4> t_backdrop_sharp: register(t2);

struct BackdropPassOutput {
    float4 position: SV_Position;
};

BackdropPassOutput backdrop_pass_vertex_impl(uint vertex_id) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    float2 position = bd_dst_rect.xy + unit_vertex * bd_dst_rect.zw;
    BackdropPassOutput output;
    output.position = float4(position / bd_dst_size * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

float4 backdrop_texel(float2 texel_pos) {
    float2 p = clamp(texel_pos, bd_src_clamp.xy, bd_src_clamp.zw) - bd_src_origin;
    return t_sprite.SampleLevel(s_backdrop, p / bd_src_size, 0);
}

BackdropPassOutput backdrop_down_vertex(uint vertex_id: SV_VertexID) {
    return backdrop_pass_vertex_impl(vertex_id);
}

float4 backdrop_down_fragment(BackdropPassOutput input): SV_Target {
    // The 2x2 block under this texel, plus four bilinear taps one source texel
    // out on the diagonals (a 4x4 tent). Wide enough that scrolling text does
    // not shimmer through the blur.
    float2 c = input.position.xy * 2.0;
    float4 sum = backdrop_texel(c) * 4.0;
    sum += backdrop_texel(c + float2(-1.0, -1.0));
    sum += backdrop_texel(c + float2(1.0, -1.0));
    sum += backdrop_texel(c + float2(-1.0, 1.0));
    sum += backdrop_texel(c + float2(1.0, 1.0));
    return sum / 8.0;
}

BackdropPassOutput backdrop_blur_vertex(uint vertex_id: SV_VertexID) {
    return backdrop_pass_vertex_impl(vertex_id);
}

float4 backdrop_blur_fragment(BackdropPassOutput input): SV_Target {
    float2 c = input.position.xy;
    float inv = -0.5 / (bd_sigma * bd_sigma);
    float4 sum = backdrop_texel(c);
    float total = 1.0;
    // Pairs of taps merged into one bilinear fetch (half the samples).
    [loop]
    for (float i = 1.0; i <= bd_radius; i += 2.0) {
        float w0 = exp(i * i * inv);
        float w1 = exp((i + 1.0) * (i + 1.0) * inv);
        float w = w0 + w1;
        float o = i + w1 / max(w, 1e-6);
        sum += (backdrop_texel(c + bd_dir * o) + backdrop_texel(c - bd_dir * o)) * w;
        total += 2.0 * w;
    }
    return sum / total;
}

struct Backdrop {
    uint order;
    float opacity;
    Bounds bounds;
    Bounds content_mask;
    Corners corner_radii;
    float blur;
    float saturation;
    float brightness;
    float bezel;
    float refraction;
    float dispersion;
    float mask;
    float4 light;
    // (rim width, band whiteness, key light, fill light) / bubble: (-, -, reflection, caustic)
    float4 rim;
    // (magnification, recess shade / bubble edge, seam line, rim gloss)
    float4 lens;
    // (0 legacy / 1 Regular / 2 Clear, interaction, light theme, material scale)
    float4 glass;
};

struct BackdropVertexOutput {
    nointerpolation uint backdrop_id: TEXCOORD0;
    float4 position: SV_Position;
    float4 clip_distance: SV_ClipDistance;
};

struct BackdropFragmentInput {
    nointerpolation uint backdrop_id: TEXCOORD0;
    float4 position: SV_Position;
};

StructuredBuffer<Backdrop> backdrops: register(t1);

BackdropVertexOutput backdrop_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint id = batch_start_index + instance_id;
    Backdrop b = backdrops[id];
    BackdropVertexOutput output;
    output.position = to_device_position(unit_vertex, b.bounds);
    output.clip_distance = distance_from_clip_rect(unit_vertex, b.bounds, b.content_mask);
    output.backdrop_id = id;
    return output;
}

// Device pixel -> blurred texel position.
float3 backdrop_sample(float2 device_pos) {
    return backdrop_texel(device_pos / bd_level_scale).rgb;
}

float3 backdrop_optical_sample(float2 device_pos, float clarity) {
    float2 p = clamp(device_pos, bd_sharp_clamp.xy, bd_sharp_clamp.zw);
    float3 sharp = t_backdrop_sharp.SampleLevel(s_backdrop, (p - bd_sharp_origin) / bd_sharp_size, 0).rgb;
    return lerp(backdrop_sample(device_pos), sharp, clarity);
}

// Outward normal of a rounded rectangle at `pt` (same construction as the
// web lens map, so corners bend along the arc and the sides stay straight).
float2 backdrop_normal(float2 pt, Bounds bounds, Corners corner_radii) {
    float2 half_size = bounds.size / 2.0;
    float2 ctp = pt - (bounds.origin + half_size);
    float r = min(pick_corner_radius(ctp, corner_radii), min(half_size.x, half_size.y));
    float2 q = abs(ctp) - (half_size - r);
    float2 s = float2(ctp.x < 0.0 ? -1.0 : 1.0, ctp.y < 0.0 ? -1.0 : 1.0);
    if (q.x > 0.0 && q.y > 0.0) {
        return normalize(q) * s;
    }
    return q.x > q.y ? float2(s.x, 0.0) : float2(0.0, s.y);
}

#include "liquid_glass.hlsl"

float4 backdrop_fragment(BackdropFragmentInput input): SV_Target {
    Backdrop b = backdrops[input.backdrop_id];
    float2 p = input.position.xy;
    float distance = quad_sdf(p, b.bounds, b.corner_radii);
    float cover = saturate(0.5 - distance) * b.opacity;
    if (b.mask > 0.5) {
        // Scroll-edge fades (the web's mask-image stops), measured from the edge.
        float t = saturate((p.y - b.bounds.origin.y) / max(b.bounds.size.y, 1.0));
        if (b.mask > 1.5) {
            t = 1.0 - t;
            cover *= t < 0.6 ? 1.0 : lerp(1.0, 0.0, (t - 0.6) / 0.4);
        } else if (t < 0.38) {
        } else if (t < 0.62) {
            cover *= lerp(1.0, 0.55, (t - 0.38) / 0.24);
        } else if (t < 0.82) {
            cover *= lerp(0.55, 0.2, (t - 0.62) / 0.2);
        } else {
            cover *= lerp(0.2, 0.0, (t - 0.82) / 0.18);
        }
    }
    if (cover <= 0.0) {
        discard;
    }

    if (b.glass.x > 0.5) {
        return float4(liquid_glass_material(b, p, distance), cover);
    }

    float3 color;
    float2 n = float2(0.0, 0.0);
    float t = 1.0;
    float2 center = b.bounds.origin + b.bounds.size / 2.0;
    // Bubble lens: magnify around the centre, then bend hard in the rim band
    // (the web lens map: (1-t)^2.2 instead of the glass rim's (1-t)^2.4).
    float zoom = max(b.lens.x, 1.0);
    bool bubble = zoom > 1.001;
    float2 base = bubble ? center + (p - center) / zoom : p;
    if (b.bezel > 0.5) {
        t = saturate(-distance / b.bezel);
        n = backdrop_normal(p, b.bounds, b.corner_radii);
        float m = bubble
            ? pow(1.0 - t, 2.2) * (0.4 + 0.6 * min(1.0, t * 5.0))
            : pow(1.0 - t, 2.4) * (0.35 + 0.65 * min(1.0, t * 6.0));
        float shift = b.refraction * 0.5 * m;
        // Total displacement: the sample is taken at `p - d`.
        float2 d = (p - base) + n * shift;
        // The curved lip is clear glass; only the flat interior is frosted.
        // Bending an already heavily blurred image erases the optical effect.
        float clarity = (1.0 - smoothstep(0.0, 0.85, t)) * 0.9;
        if (length(d) * b.dispersion > 0.35) {
            color.r = backdrop_optical_sample(p - d, clarity).r;
            color.g = backdrop_optical_sample(p - d * (1.0 - b.dispersion * 0.5), clarity).g;
            color.b = backdrop_optical_sample(p - d * (1.0 - b.dispersion), clarity).b;
        } else {
            color = backdrop_optical_sample(p - d, clarity);
        }
    } else {
        color = backdrop_sample(base);
    }

    // CSS saturate() and brightness(), in that order, on sRGB values.
    float s = b.saturation;
    float3x3 sat = float3x3(
        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s,
        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s,
        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s);
    color = saturate(mul(sat, color) * b.brightness);

    // Rim light: a specular line on the edges that face the light (top left),
    // a dimmer one opposite, a soft inner glow, and shade at the bottom.
    if (b.bezel > 0.5 && any(b.light > 0.0)) {
        float2 l = float2(-0.45, -0.89);
        float dir = dot(n, l);
        float top = max(dir, 0.0);
        float bottom = max(-dir, 0.0);
        float depth = max(-distance, 0.0);
        float edge = depth < 7.0 ? exp(-depth / 1.3) : 0.0;
        float spec = b.light.x * top * sqrt(top) * edge + b.light.y * bottom * sqrt(bottom) * edge;
        float glow = b.light.z * top * top * pow(1.0 - t, 2.2);
        float white = saturate(spec + glow);
        float shade = b.light.w * bottom * (1.0 - t) * (1.0 - t);
        color = (color * (1.0 - white) + white) * (1.0 - shade);
    }

    float depth = max(-distance, 0.0);
    float3 ink = float3(0.078, 0.118, 0.235);
    if (b.rim.x > 0.5 || bubble) {
        float2 nr = backdrop_normal(p, b.bounds, b.corner_radii);
        float top = max(-nr.y, 0.0);
        float bot = max(nr.y, 0.0);
        float lw = max(1.0, b.rim.x * 0.3);
        if (bubble) {
            // A glass bubble: studio reflection at the top left, light gathered
            // at the bottom, a dark Fresnel edge and a bright lip on top.
            float2 uv = (p - b.bounds.origin) / max(b.bounds.size, float2(1.0, 1.0));
            float spec = saturate(1.0 - length((uv - float2(0.28, 0.22)) / float2(0.38, 0.44)) / 0.72);
            float caus = saturate(1.0 - length((uv - float2(0.56, 1.08)) / float2(0.64, 0.46)) / 0.74);
            color = lerp(color, float3(1.0, 1.0, 1.0), saturate(spec * b.rim.z + caus * b.rim.w));
            float edge = exp(-depth / max(1.5, b.bezel * 0.16));
            color = lerp(color, ink, edge * b.lens.y);
            float lip = saturate(1.0 - depth / 1.5);
            color = lerp(color, float3(1.0, 1.0, 1.0), lip * (0.85 * sqrt(top) + 0.35 * sqrt(bot)) * b.rim.z);
        } else {
            // Continuous curved glass, rather than a constant white bevel
            // and a dark seam. Fresnel reflection increases toward the edge.
            float w = b.rim.x;
            float curve = exp(-depth / max(w, 1.0));
            float fresnel = pow(curve, 3.0);
            float key = pow(max(dot(nr, normalize(float2(-0.6, -0.8))), 0.0), 4.0);
            float fill = pow(max(dot(nr, normalize(float2(0.6, 0.8))), 0.0), 5.0);
            float reflection = fresnel * (b.rim.y + b.lens.w * key);
            color = lerp(color, float3(1.0, 1.0, 1.0), saturate(reflection));
            float caustic = exp(-pow((depth - w * 1.1) / max(w * 0.55, 1.0), 2.0));
            color += float3(0.85, 0.94, 1.0) * caustic * fill * b.rim.w * 0.28;
            color *= 1.0 - b.lens.y * curve * (1.0 - key) * 0.25;
            float lip = saturate(1.0 - depth / lw);
            color = lerp(color, float3(1.0, 1.0, 1.0), saturate(lip * (b.rim.z * sqrt(top) + b.rim.w * sqrt(bot))));
            // A broad, faint glass sheen across the window (CSS: 112deg, peak at 47%).
            float2 dir = float2(0.927, 0.375);
            float len = abs(b.bounds.size.x * dir.x) + abs(b.bounds.size.y * dir.y);
            float tt = 0.5 + dot(p - center, dir) / max(len, 1.0);
            float sh = saturate(1.0 - abs(tt - 0.47) / 0.1);
            color = lerp(color, float3(1.0, 1.0, 1.0), sh * b.rim.y * 0.4);
        }
    }
    return float4(color, cover);
}
